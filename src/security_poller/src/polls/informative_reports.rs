use std::collections::HashMap;

use graphql_client::GraphQLQuery;
use security_api::{
    hackerone::{self, HackerOneClient},
    models::{self, UserThanksData},
    redis::{self, redis::AsyncCommands},
};

use crate::polls::reputation;

use super::PollConfiguration;

pub async fn run_poll(config: &PollConfiguration) -> Result<(), anyhow::Error> {
    debug!("running poll");

    let mut kv = redis::get_connection().get().await?;
    let last_saved_thanks_data = get_saved_thanks_data().await?;
    let leaderboard = reputation::get_saved_reputation_data().await?;
    if leaderboard.is_none() {
        return Ok(());
    }

    let leaderboard = leaderboard.unwrap();
    let last_saved_thanks_data = last_saved_thanks_data.unwrap_or_default();
    let mut thanks_data = vec![];

    // Fetch new Thanks data
    let hackerone_program = config.team_handle.as_ref().map(|x| x.as_str());
    for user in leaderboard {
        let username = &user.user_name;
        let mut user_thanks =
            hackerone_get_user_thanks_data(&username, &config.hackerone, hackerone_program).await?;
        thanks_data.append(&mut user_thanks);
    }

    // Diff
    let mut changes = vec![];
    for previous_thanks_data in last_saved_thanks_data {
        let new_data = thanks_data.iter().find(|t| {
            t.user_id == previous_thanks_data.user_id
                && t.team_handle == previous_thanks_data.team_handle
        });

        if let Some(new_data) = new_data {
            if new_data.invalid_report_count > previous_thanks_data.invalid_report_count {
                let invalid_report_change = new_data.invalid_report_count - previous_thanks_data.invalid_report_count;
                let change = models::UserInvalidReportChange {
                    user_id: new_data.user_id.clone(),
                    user_name: new_data.user_name.clone(),
                    invalid_reports: invalid_report_change,
                    team_handle: new_data.team_handle.clone(),
                };

                trace!("found change: {change:?}");
                changes.push(change);
            }
        }
    }

    // Group
    let mut changes_grouped = HashMap::new();
    for change in changes {
        let team_handle = &change.team_handle;
        let group = if changes_grouped.contains_key(team_handle) {
            changes_grouped.get_mut(team_handle).unwrap()
        } else {
            changes_grouped.insert(String::from(team_handle), vec![]);
            changes_grouped.get_mut(team_handle).unwrap()
        };

        group.push(change);
    }

    // Queue
    let changes = changes_grouped.len();
    if !changes_grouped.is_empty() {
        for (team_handle, changes) in changes_grouped {
            let queue_item = models::InvalidReportsQueueItem {
                changes,
                team_handle,
            };
    
            let queue_item_encoded = serde_json::to_string(&queue_item)?;
            kv.publish::<&str, std::string::String, i32>(
                models::redis_keys::USER_INFORMATIVE_REPORTS_QUEUE_PUBSUB,
                queue_item_encoded,
            ).await?;
        }
    }

    // Save new data
    redis::save_vec_to_set(
        models::redis_keys::USER_THANKS_DATA_POLL_LAST_DATA,
        thanks_data,
        true,
        &mut kv,
    ).await?;
    info!("ran poll, {} changes", changes);
    Ok(())
}

async fn get_saved_thanks_data() -> Result<Option<Vec<models::UserThanksData>>, anyhow::Error> {
    let mut kv = redis::get_connection().get().await?;
    let last_thanks_data =
        redis::load_set_to_vec(models::redis_keys::USER_THANKS_DATA_POLL_LAST_DATA, &mut kv)
            .await?;

    let mut data = vec![];
    if last_thanks_data.is_empty() {
        return Ok(None);
    }

    for d in last_thanks_data {
        let deserialized = serde_json::from_str::<models::UserThanksData>(&d)?;
        data.push(deserialized);
    }

    Ok(Some(data))
}

async fn hackerone_get_user_thanks_data(
    username: &str,
    client: &HackerOneClient,
    hackerone_program: Option<&str>,
) -> Result<Vec<models::UserThanksData>, anyhow::Error> {
    let variables = hackerone::user_profile_thanks::Variables {
        username: String::from(username),
        page_size: 100,
    };

    let query = hackerone::UserProfileThanks::build_query(variables);
    let response = client
        .http
        .post("https://hackerone.com/graphql")
        .json(&query)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::Error::msg("HackerOne API returned bad status code"));
    }

    let mut thanks: Vec<models::UserThanksData> = vec![];

    let data = response.json::<graphql_client::Response<<hackerone::UserProfileThanks as GraphQLQuery>::ResponseData>>().await?;
    trace!("{} {:?}", username, data);
    if let Some(errors) = data.errors {
        if !errors.is_empty() {
            return Err(anyhow::Error::msg(errors.first().unwrap().message.clone()));
        }
    }

    let user = data.data.as_ref().unwrap().user.as_ref().unwrap();
    let thanks_items = user.thanks_items.as_ref().unwrap().edges.as_ref().unwrap();
    for program_thanks_data in thanks_items {
        if let Some(program_thanks_data) = program_thanks_data {
            let thanks_item = &program_thanks_data.node.as_ref().unwrap().thanks_item;
            if let Some(team) = &thanks_item.team {
                if let Some(hackerone_program) = &hackerone_program {
                    if team.handle != *hackerone_program {
                        continue;
                    }
                }

                let report_count = thanks_item.report_count.unwrap_or(0);
                let total_report_count = thanks_item.total_report_count.unwrap_or(0);
                let reputation = thanks_item.reputation.unwrap_or_default();

                let user_thanks_data = UserThanksData {
                    user_id: user.id.clone(),
                    user_name: user.username.clone(),
                    team_handle: team.handle.clone(),
                    resolved_report_count: report_count,
                    invalid_report_count: total_report_count - report_count,
                    total_report_count,
                    reputation,
                };

                thanks.push(user_thanks_data);
            }
        }
    }

    return Ok(thanks);
}

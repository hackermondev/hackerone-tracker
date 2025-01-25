use std::collections::HashMap;

use super::PollConfiguration;
extern crate cronjob;
use chrono;
use graphql_client::GraphQLQuery;
use security_api::hackerone::{self as hackerone, HackerOneClient};
use security_api::models::{self as models};
use security_api::redis::{self, redis::AsyncCommands};

pub async fn run_poll(config: &PollConfiguration) -> Result<(), anyhow::Error> {
    debug!("running poll");

    let mut kv = redis::get_connection().get().await?;
    let last_run_time: Option<String> = kv.get(models::redis_keys::REPUTATION_QUEUE_LAST_RUN_TIME).await?;
    let previous_reputation_save = get_old_reputation_data().await?;

    let mut programs = vec![];
    if let Some(team_handle) = &config.team_handle {
        programs.push(team_handle.to_owned());
    } else {
        // Get reputation data from all programs
        let mut _programs =
            redis::load_set_to_vec(models::redis_keys::PROGRAMS, &mut kv).await?;
        programs.append(&mut _programs);
    }

    debug!("getting rep data for {} programs", programs.len());

    let mut leaderboard = vec![];
    let single_program = programs.len() == 1;

    for program in programs {
        let mut program_leaderboard = hackerone_get_leaderboard(&program, &config.hackerone, true, None, None).await?;
        leaderboard.append(&mut program_leaderboard);
    }


    // First Run
    if last_run_time.is_none() || previous_reputation_save.is_none() {
        redis::save_vec_to_set(
            models::redis_keys::REPUTATION_QUEUE_LAST_DATA,
            leaderboard,
            true,
            &mut kv,
        ).await?;
        set_last_run_time_now().await?;
        return Ok(());
    }


    let mut changed: Vec<Vec<models::RepData>> = Vec::new();

    // Create a hashmap for quick lookup of last_rep_data
    let mut last_rep_map: HashMap<(String, Option<String>), models::RepData> = HashMap::new();
    if let Some(ref last_rep_data_vec) = previous_reputation_save {
        for data in last_rep_data_vec {
            last_rep_map.insert(
                (data.user_id.clone(), data.team_handle.clone()),
                data.clone(),
            );
        }
    }

    // Process current rep_data
    for rep in &leaderboard {
        let key = (rep.user_id.clone(), rep.team_handle.clone());
        if let Some(old_data) = last_rep_map.remove(&key) {
            if old_data.reputation != rep.reputation {
                changed.push(vec![old_data.clone(), rep.clone()]);
            }
        } else {
            // User was added
            let empty = models::RepData {
                reputation: -1,
                rank: -1,
                user_name: "".into(),
                team_handle: None,
                user_profile_image_url: "".into(),
                user_id: "".into(),
            };

            changed.push(vec![empty, rep.clone()]);
        };

        drop(key);
    }

    // Process remaining last_rep_data, these users were removed
    if previous_reputation_save.is_some() {
        for remaining in last_rep_map.values() {
            let empty = models::RepData {
                reputation: -1,
                rank: -1,
                ..Default::default()
            };

            changed.push(vec![remaining.clone(), empty]);
        }
    }

    debug!("reputation poll event: changed len: {}", changed.len());
    if !changed.is_empty() {
        let mut queue_item = models::RepDataQueueItem {
            id: None,
            diff: changed.clone(),
            created_at: chrono::Utc::now().naive_utc(),
            include_team_handle: !single_program,
        };

        queue_item.create_id();
        let queue_item_encoded = serde_json::to_string(&queue_item).unwrap();
        kv.publish::<&str, std::string::String, i32>(
            models::redis_keys::REPUTATION_QUEUE_PUBSUB,
            queue_item_encoded,
        ).await?;
        add_queue_item_to_backlog(&queue_item).await?;
    }

    redis::save_vec_to_set(
        models::redis_keys::REPUTATION_QUEUE_LAST_DATA,
        leaderboard,
        true,
        &mut kv,
    ).await?;
    set_last_run_time_now().await?;

    info!("reputation: ran poll, {} changes", changed.len());
    Ok(())
}

async fn set_last_run_time_now() -> Result<(), anyhow::Error> {
    let mut kv = redis::get_connection().get().await?;
    let now = chrono::Utc::now();
    let ms = now.timestamp_millis();

    kv.set::<_, _, ()>(models::redis_keys::REPUTATION_QUEUE_LAST_RUN_TIME, ms).await?;
    Ok(())
}

#[rustfmt::skip]
async fn hackerone_get_leaderboard(handle: &str, client: &HackerOneClient, get_full_leaderboard: bool, previous_data: Option<Vec<models::RepData>>, next_cursor: Option<String>) -> Result<Vec<models::RepData>, anyhow::Error> {
    debug!("get reputation data {}, cursor: {}", handle, next_cursor.as_ref().unwrap_or(&String::from("")));
    let variables = hackerone::team_year_thank_query::Variables {
        selected_handle: handle.to_string(),
        year: None,
        cursor: next_cursor.unwrap_or(String::from("")),
    };

    let query = hackerone::TeamYearThankQuery::build_query(variables);
    let response = client.http.post("https://hackerone.com/graphql").json(&query).send().await?;

    let mut result: Vec<models::RepData> = vec![];
    if previous_data.is_some() {
        result = previous_data.unwrap();
    }

    if !response.status().is_success() {
        return Err(anyhow::Error::msg("HackerOne API returned bad status code"))
    }
    
    let data = response.json::<graphql_client::Response<<hackerone::TeamYearThankQuery as GraphQLQuery>::ResponseData>>().await?;
    trace!("{} {:?}", handle, data);
    if let Some(errors) = data.errors {
        if !errors.is_empty() {
            return Err(anyhow::Error::msg(errors.first().unwrap().message.clone()));
        }
    }

    let team_handle = data.data.as_ref().unwrap().selected_team.as_ref().unwrap().handle.clone();
    let participants = &data.data.as_ref().unwrap().selected_team.as_ref().unwrap().participants;
    if participants.is_none() {
        warn!("{} returned no participants", handle);
        return Ok(vec![])
    }

    let participants = participants.as_ref().unwrap();
    let page_info = &participants.page_info; // rustfmt::skip
    let researchers = participants.edges.as_ref().unwrap();

    for researcher in researchers {
        let user = researcher.as_ref().unwrap().node.as_ref().unwrap();
        let reputation = researcher.as_ref().unwrap().top_participant_participant.reputation.unwrap_or(0);
        let rank = researcher.as_ref().unwrap().rank.unwrap_or(-1);

        let data = models::RepData {
            reputation,
            rank,
            user_name: user.username.clone(),
            user_id: user.database_id.clone(),
            user_profile_image_url: user.profile_picture.clone(),
            team_handle: Some(team_handle.clone()),
        };

        result.push(data);
    }

    if page_info.has_next_page && get_full_leaderboard {
        let end_cursor = page_info.end_cursor.as_ref().unwrap();
        let next_page_reputation_data = Box::pin(hackerone_get_leaderboard(handle, client, true, Some(result), Some(end_cursor.clone()))).await?;
        return Ok(next_page_reputation_data);
    }

    debug!("{} researches in {handle}: {result:?}", result.len());
    Ok(result)
}

async fn get_old_reputation_data() -> Result<Option<Vec<models::RepData>>, anyhow::Error> {
    let mut kv = redis::get_connection().get().await?;
    let last_rep_data = redis::load_set_to_vec(
        models::redis_keys::REPUTATION_QUEUE_LAST_DATA,
        &mut kv,
    ).await?;

    let mut data: Vec<models::RepData> = vec![];
    if last_rep_data.is_empty() {
        return Ok(None);
    }

    for d in last_rep_data {
        let deserialized: models::RepData = serde_json::from_str::<models::RepData>(&d).unwrap();
        data.push(deserialized);
    }

    Ok(Some(data))
}

async fn add_queue_item_to_backlog(item: &models::RepDataQueueItem) -> Result<(), anyhow::Error> {
    let mut kv = redis::get_connection().get().await?;
    let serialized = serde_json::to_string(item).unwrap();
    let now = chrono::Utc::now().timestamp_millis();

    kv.zadd::<_, _, _, ()>(models::redis_keys::REPUTATION_QUEUE_BACKLOG, serialized, now).await?;
    Ok(())
}

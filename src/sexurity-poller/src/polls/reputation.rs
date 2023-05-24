use super::PollConfiguration;
extern crate cronjob;
use cronjob::CronJob;
use chrono;
use chrono::Datelike;
use graphql_client::GraphQLQuery;
use sexurity_api::hackerone::{self as hackerone, HackerOneClient};
use sexurity_api::models::{self as models};
use sexurity_api::redis::redis::Commands;
use sexurity_api::redis::{load_set_to_vec, redis, redis::cmd, save_vec_to_set};

pub fn start_poll_event_loop(config: &PollConfiguration) {
    let poll_config = config.clone();
    let mut cron = CronJob::new("reputation_poll", move |_name: &str| {
        run_poll(&poll_config).unwrap();
    });

    // Every 5 minutes
    cron.minutes("*/5");
    cron.seconds("0");
    CronJob::start_job_threaded(cron);
}

pub fn run_poll(config: &PollConfiguration) -> Result<(), Box<dyn std::error::Error>> {
    let mut redis_conn = config.redis_client.get_connection()?;
    let last_run_time: Option<String> = cmd("GET")
        .arg("reputation_poll_last_run_time")
        .query(&mut redis_conn)?;
    let mut last_rep_data = get_old_reputation_data(&mut redis_conn);
    let rep_data = get_reputation_data(&config.team_handle, &config.hackerone).unwrap();

    if last_run_time.is_none() || last_rep_data.is_none() {
        // first run
        save_vec_to_set(
            "reputation_poll_last_data".to_string(),
            rep_data,
            &mut redis_conn,
        )?;
        set_last_run_time_now(&mut redis_conn);
        return Ok(());
    }

    let mut changed: Vec<Vec<models::RepData>> = Vec::new();
    let rep_data_cloned = rep_data.clone();
    for rep in rep_data {
        let user_id = rep.user_id.clone();
        let old_data = last_rep_data
            .as_ref()
            .unwrap()
            .into_iter()
            .find(|p| p.user_id == user_id);

        if old_data.is_none() {
            // user was added
            let empty = models::RepData {
                reputation: -1,
                rank: -1,
                user_name: "".into(),
                user_profile_image_url: "".into(),
                user_id: "".into(),
            };

            let diff: Vec<models::RepData> = vec![empty, rep];
            changed.push(diff);
        } else {
            if old_data.unwrap().reputation != rep.reputation {
                let diff: Vec<models::RepData> = vec![old_data.unwrap().clone(), rep.clone()];
                changed.push(diff);
            }

            let index = last_rep_data
                .as_ref()
                .unwrap()
                .into_iter()
                .position(|d| d.user_id == rep.user_id)
                .unwrap();

            last_rep_data.as_mut().unwrap().remove(index);
        }
    }

    let last_rep_data_unwrapped = last_rep_data.unwrap();
    if last_rep_data_unwrapped.len() > 0 {
        // User was removed
        for rep in last_rep_data_unwrapped {
            let empty = models::RepData {
                reputation: -1,
                rank: -1,
                user_name: "".into(),
                user_profile_image_url: "".into(),
                user_id: "".into(),
            };

            let diff: Vec<models::RepData> = vec![rep, empty];
            changed.push(diff);
        }
    }

    if changed.len() > 0 {
        let mut queue_item = models::RepDataQueueItem {
            id: None,
            team_handle: config.team_handle.clone(),
            diff: changed,
            created_at: chrono::Utc::now(),
        };

        queue_item.create_id();
        let queue_item_encoded = serde_json::to_string(&queue_item).unwrap();
        redis_conn.publish::<&str, std::string::String, i32>(
            "reputation_poll_queue",
            queue_item_encoded,
        )?;
        add_queue_item_to_backlog(&queue_item, &mut redis_conn);
    }

    save_vec_to_set(
        "reputation_poll_last_data".to_string(),
        rep_data_cloned,
        &mut redis_conn,
    )?;
    set_last_run_time_now(&mut redis_conn);
    Ok(())
}

fn set_last_run_time_now(conn: &mut redis::Connection) {
    let now = chrono::Utc::now();
    let ms = now.timestamp_millis();

    cmd("SET")
        .arg("reputation_poll_last_run_time")
        .arg(ms)
        .query::<String>(conn)
        .unwrap();
}

fn get_reputation_data(
    handle: &str,
    client: &HackerOneClient,
) -> Result<Vec<models::RepData>, Box<dyn std::error::Error>> {
    let now = chrono::Utc::now().date_naive();
    let variables = hackerone::team_year_thank_query::Variables {
        selected_handle: handle.to_string(),
        year: Some(now.year().into()),
    };

    let query = hackerone::TeamYearThankQuery::build_query(variables);
    let response = client
        .http
        .post("https://hackerone.com/graphql")
        .json(&query)
        .send()?;

    let mut result: Vec<models::RepData> = vec![];
    // (TODO): find a better way to do this?
    let data = response.json::<graphql_client::Response<<hackerone::TeamYearThankQuery as GraphQLQuery>::ResponseData>>().unwrap();
    let researchers = data
        .data
        .unwrap()
        .selected_team
        .unwrap()
        .participants
        .unwrap()
        .edges
        .unwrap();
    for researcher in researchers {
        let user = researcher.as_ref().unwrap().node.as_ref().unwrap();
        let reputation = researcher
            .as_ref()
            .unwrap()
            .top_participant_participant
            .reputation
            .unwrap_or(0);
        let rank = researcher.as_ref().unwrap().rank.unwrap_or(-1);

        let data = models::RepData {
            reputation,
            rank,
            user_name: user.username.clone(),
            user_id: user.database_id.clone(),
            user_profile_image_url: user.profile_picture.clone(),
        };

        result.push(data);
    }

    Ok(result)
}

fn get_old_reputation_data(conn: &mut redis::Connection) -> Option<Vec<models::RepData>> {
    let last_rep_data =
        load_set_to_vec(String::from("reputation_poll_last_data"), conn).unwrap_or(vec![]);
    let mut data: Vec<models::RepData> = vec![];

    if last_rep_data.len() == 0 {
        return None;
    }

    for d in last_rep_data {
        let deserialized: models::RepData = serde_json::from_str::<models::RepData>(&d).unwrap();
        data.push(deserialized);
    }

    Some(data)
}

fn add_queue_item_to_backlog(item: &models::RepDataQueueItem, conn: &mut redis::Connection) {
    let serialized = serde_json::to_string(item).unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    cmd("ZADD")
        .arg("reputation_queue")
        .arg(now)
        .arg(serialized)
        .query::<i64>(conn)
        .unwrap();
}

use super::PollConfiguration;
extern crate cronjob;
use chrono;
use cronjob::CronJob;
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
    info!("reputation: started poll event loop");
}

pub fn run_poll(config: &PollConfiguration) -> Result<(), Box<dyn std::error::Error>> {
    debug!("reputation poll event: running poll");
    let mut redis_conn = config.redis_client.get_connection()?;
    let last_run_time: Option<String> = cmd("GET")
        .arg(models::redis_keys::REPUTATION_QUEUE_LAST_RUN_TIME)
        .query(&mut redis_conn)?;
    let mut last_rep_data = get_old_reputation_data(&mut redis_conn);
    let rep_data = get_reputation_data(&config.team_handle, &config.hackerone, None, None);
    if rep_data.is_err() {
        error!("reputation poll event: error getting reputation data: {}", rep_data.err().unwrap());
        return Ok(());
    }

    let rep_data = rep_data.unwrap();
    debug!(
        "reputation poll event: last_run_time {}",
        last_run_time.clone().unwrap_or("-1".into())
    );
    debug!(
        "reputation poll event: last_rep_data len: {}, current rep_data len: {}",
        last_rep_data.clone().unwrap_or(vec![]).len(),
        rep_data.len()
    );

    if last_run_time.is_none() || last_rep_data.is_none() {
        // first run
        save_vec_to_set(
            models::redis_keys::REPUTATION_QUEUE_LAST_DATA.to_string(),
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

    debug!("reputation poll event: changed len: {}", changed.len());
    if changed.len() > 0 {
        let mut queue_item = models::RepDataQueueItem {
            id: None,
            team_handle: config.team_handle.clone(),
            diff: changed.clone(),
            created_at: chrono::Utc::now(),
        };

        queue_item.create_id();
        let queue_item_encoded = serde_json::to_string(&queue_item).unwrap();
        redis_conn.publish::<&str, std::string::String, i32>(
            models::redis_keys::REPUTATION_QUEUE_PUBSUB,
            queue_item_encoded,
        )?;
        add_queue_item_to_backlog(&queue_item, &mut redis_conn);
    }

    save_vec_to_set(
        models::redis_keys::REPUTATION_QUEUE_LAST_DATA.to_string(),
        rep_data_cloned,
        &mut redis_conn,
    )?;
    set_last_run_time_now(&mut redis_conn);
    info!("reputation: ran poll, {} changes", changed.len());

    Ok(())
}

fn set_last_run_time_now(conn: &mut redis::Connection) {
    let now = chrono::Utc::now();
    let ms = now.timestamp_millis();

    cmd("SET")
        .arg(models::redis_keys::REPUTATION_QUEUE_LAST_RUN_TIME)
        .arg(ms)
        .query::<String>(conn)
        .unwrap();
}

#[cfg_attr(rustfmt, rustfmt_skip)]
fn get_reputation_data(handle: &str, client: &HackerOneClient, previous_data: Option<Vec<models::RepData>>, next_cursor: Option<String>) -> Result<Vec<models::RepData>, Box<dyn std::error::Error>> {
    debug!("get reputation data, cursor: {}", next_cursor.as_ref().unwrap_or(&String::from("")));
    let variables = hackerone::team_year_thank_query::Variables {
        selected_handle: handle.to_string(),
        year: None,
        cursor: next_cursor.unwrap_or(String::from("")),
    };

    let query = hackerone::TeamYearThankQuery::build_query(variables);
    let response = client.http.post("https://hackerone.com/graphql").json(&query).send()?;

    let mut result: Vec<models::RepData> = vec![];
    if previous_data.is_some() {
        result = previous_data.unwrap();
    }

    if !response.status().is_success() {
        return Err("HackerOne API returned bad status code".into())
    }
    
    let data = response.json::<graphql_client::Response<<hackerone::TeamYearThankQuery as GraphQLQuery>::ResponseData>>()?;
    let page_info = &data.data.as_ref().unwrap().selected_team.as_ref().unwrap().participants.as_ref().unwrap().page_info; // rustfmt::skip
    let researchers = data.data.as_ref().unwrap().selected_team.as_ref().unwrap().participants.as_ref().unwrap().edges.as_ref().unwrap();

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
        };

        result.push(data);
    }

    if page_info.has_next_page {
        let end_cursor = page_info.end_cursor.as_ref().unwrap();
        return Ok(get_reputation_data(handle, client, Some(result), Some(end_cursor.clone())).unwrap());
    }

    Ok(result)
}

fn get_old_reputation_data(conn: &mut redis::Connection) -> Option<Vec<models::RepData>> {
    let last_rep_data = load_set_to_vec(
        String::from(models::redis_keys::REPUTATION_QUEUE_LAST_DATA),
        conn,
    )
    .unwrap_or(vec![]);
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
        .arg(models::redis_keys::REPUTATION_QUEUE_BACKLOG)
        .arg(now)
        .arg(serialized)
        .query::<i64>(conn)
        .unwrap();
}

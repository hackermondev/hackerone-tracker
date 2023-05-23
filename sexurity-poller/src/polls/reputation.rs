use super::PollConfiguration;
use chrono;
use chrono::Datelike;
use graphql_client::GraphQLQuery;
use serde::{Deserialize, Serialize};
use sexurity_api::hackerone::{self as hackerone, HackerOneClient};
use sexurity_api::redis::{load_set_to_vec, redis, redis::cmd, save_vec_to_set};

#[derive(Debug, Deserialize, Serialize)]
struct RepData {
    reputation: i64,
    rank: i64,
    user_name: String,
    user_profile_image_url: String,
    user_id: String,
}

pub fn run_poll(config: &PollConfiguration) -> Result<(), Box<dyn std::error::Error>> {
    let mut redis_conn = config.redis_client.get_connection()?;
    let last_run_time: Option<String> = cmd("GET")
        .arg("reputation_poll_last_run_time")
        .query(&mut redis_conn)?;
    let last_rep_data = get_old_reputation_data(&mut redis_conn);

    let data = get_reputation_data(&config.team_handle, &config.hackerone).unwrap();
    if last_run_time.is_none() || last_rep_data.is_none() {
        // first run
        save_vec_to_set(
            "reputation_poll_last_data".to_string(),
            data,
            &mut redis_conn,
        )?;
        set_last_run_time_now(&mut redis_conn);
        return Ok(());
    }

    println!("{:#?}", last_rep_data);
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
) -> Result<Vec<RepData>, Box<dyn std::error::Error>> {
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

    let mut result: Vec<RepData> = vec![];
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

        let data = RepData {
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

fn get_old_reputation_data(conn: &mut redis::Connection) -> Option<Vec<RepData>> {
    let last_rep_data =
        load_set_to_vec(String::from("reputation_poll_last_data"), conn).unwrap_or(vec![]);
    let mut data: Vec<RepData> = vec![];

    if last_rep_data.len() == 0 {
        return None;
    }

    for d in last_rep_data {
        let deserialized: RepData = serde_json::from_str::<RepData>(&d).unwrap();
        data.push(deserialized);
    }

    Some(data)
}

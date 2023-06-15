#![allow(deprecated)]
extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod polls;
use std::thread;
use std::time::Duration;

use chrono;
use chrono::Datelike;
use clap::Parser;

// use std::{env, ffi::OsString, str::FromStr};
use graphql_client::GraphQLQuery;
use sexurity_api::hackerone::{self as hackerone, HackerOneClient};
use sexurity_api::redis;

use crate::polls::PollConfiguration;

#[derive(Default, Debug, Parser)]
#[clap(author = "hackermon", version, about)]
struct Arguments {
    #[arg(short = 'T', long = "session_token", env = "SESSION_TOKEN")]
    hackerone_session_token: Option<String>,

    #[arg(short = 'R', long = "redis", env = "REDIS_URL")]
    redis: String,

    #[arg(short = 'H', long = "handle")]
    hackerone_handle: String,

    #[arg(default_value = "false", long)]
    disable_reputation_polling: bool,

    #[arg(default_value = "false", long)]
    disable_hackactivity_polling: bool,

    #[arg(default_value = "false", long)]
    disable_user_report_count_polling: bool,
}
fn main() {
    pretty_env_logger::init();
    let args = Arguments::parse();
    info!("hello world");
    debug!("hackerone team handle: {}", args.hackerone_handle);
    debug!("{:#?}", args);

    let session_token = args.hackerone_session_token.clone().unwrap_or("".into());
    let csrf_token = hackerone::get_hackerone_csrf_token(&session_token).unwrap();
    debug!("csrf_token: {}", csrf_token);

    let client = hackerone::HackerOneClient::new(csrf_token, session_token.to_string());
    let good_args = ensure_args(&client, &args);
    if !good_args {
        panic!("cannot fetch team. ensure your session token is valid and the team name is valid and your session token is in the team (if its private)")
    }

    let redis = redis::open(args.redis.as_ref()).unwrap();
    let config = PollConfiguration {
        hackerone: client,
        team_handle: args.hackerone_handle.clone(),
        redis_client: redis,
    };

    polls::reputation::run_poll(&config).unwrap();
    polls::reports::run_poll(&config).unwrap();

    polls::reputation::start_poll_event_loop(&config);
    polls::reports::start_poll_event_loop(&config);

    // keep main thread alive
    loop {
        thread::sleep(Duration::from_secs(60 * 100));
    }
}

fn ensure_args(client: &HackerOneClient, args: &Arguments) -> bool {
    let now = chrono::Utc::now().date_naive();

    // Verify HackerOne handle
    let variables = hackerone::team_year_thank_query::Variables {
        selected_handle: args.hackerone_handle.clone(),
        year: Some(now.year().into()),
        cursor: "".into(),
    };

    let query = hackerone::TeamYearThankQuery::build_query(variables);
    let response = client
        .http
        .post("https://hackerone.com/graphql")
        .json(&query)
        .send()
        .unwrap();

    let data = response.json::<graphql_client::Response<<hackerone::TeamYearThankQuery as GraphQLQuery>::ResponseData>>().unwrap();
    let can_fetch_team = data
        .data
        .expect("Response data not found")
        .selected_team
        .is_some();

    return can_fetch_team;
}

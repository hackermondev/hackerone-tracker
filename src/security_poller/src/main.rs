#![allow(deprecated)]
extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod polls;
use std::env;
use std::time::Duration;

use chrono::Datelike;
use clap::Parser;

use graphql_client::GraphQLQuery;
use security_api::hackerone::{self as hackerone, HackerOneClient};
use tokio::sync::mpsc;

use crate::polls::PollConfiguration;

#[derive(Default, Debug, Parser)]
#[clap(author = "hackermon", version, about)]
struct Arguments {
    #[arg(short = 'T', long = "session_token", env = "SESSION_TOKEN")]
    hackerone_session_token: Option<String>,

    #[arg(short = 'R', long = "redis", env = "REDIS_URL")]
    redis: String,

    #[arg(short = 'H', long = "handle")]
    hackerone_handle: Option<String>,

    #[arg(default_value = "true", long)]
    reputation_polling: bool,

    #[arg(default_value = "true", long)]
    hackactivity_polling: bool,
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    let args = Arguments::parse();
    info!("hello world");
    debug!("hackerone team handle: {:?}", args.hackerone_handle);
    debug!("{:#?}", args);

    let session_token = args.hackerone_session_token.clone().unwrap_or("".into());
    let csrf_token = hackerone::fetch_csrf_token(&session_token).await.unwrap();
    debug!("csrf_token: {}", csrf_token);

    let client = hackerone::HackerOneClient::new(csrf_token, session_token.to_string());
    let good_args = ensure_args(&client, &args).await.unwrap();
    if !good_args {
        panic!("cannot fetch team. ensure your session token is valid and the team name is valid and your session token is in the team (if its private)")
    }

    let redis_url = &args.redis;
    env::set_var("REDIS_URL", redis_url);
    
    let mut tasks = vec![];
    let is_tracking_all_programs = args.hackerone_handle.is_none();
    let config = PollConfiguration {
        hackerone: client,
        team_handle: args.hackerone_handle,
    };

    if is_tracking_all_programs {
        let config = config.clone();
        let program_tracking_task = tokio::spawn(async move {
            let interval = Duration::from_secs(60 * 60 * 12); // 12 hours
            loop {
                if let Err(err) = polls::programs::run_poll(&config).await {
                    error!("programs: {}", err);
                }

                tokio::time::sleep(interval).await;
            }
        });

        tasks.push(program_tracking_task);
    }

    if args.reputation_polling {
        let config = config.clone();
        let leaderboard_tracking_task = tokio::spawn(async move {
            let interval = Duration::from_secs(60); // 1 minute
            loop {
                if let Err(err) = polls::reputation::run_poll(&config).await {
                    error!("reputation: {}", err);
                }

                tokio::time::sleep(interval).await;
            }
        });

        tasks.push(leaderboard_tracking_task);
    }
    
    if args.hackactivity_polling {
        let config = config.clone();
        let reports_tracking_task = tokio::spawn(async move {
            let interval = Duration::from_secs(60 * 5); // 5 minutes
            loop {
                if let Err(err) = polls::reports::run_poll(&config).await {
                    error!("reports: {}", err);
                }
        
                tokio::time::sleep(interval).await;
            }
        });

        tasks.push(reports_tracking_task);
    }

    // Wait for any task to abort
    let (abort_sender, mut abort_receiver) = mpsc::channel(1);
    for task in tasks {
        let sender = abort_sender.clone();
        tokio::spawn(async move {
            let result = task.await;
            error!("task aborted {result:?}");
            let _ = sender.send(()).await;
        });
    }

    let _ = abort_receiver.recv().await;
}

async fn ensure_args(client: &HackerOneClient, args: &Arguments) -> Result<bool, anyhow::Error> {
    let now = chrono::Utc::now().date_naive();

    // Verify HackerOne handle
    if let Some(hackerone_handle) = &args.hackerone_handle {
        let variables = hackerone::team_year_thank_query::Variables {
            selected_handle: hackerone_handle.clone(),
            year: Some(now.year().into()),
            cursor: "".into(),
        };

        let query = hackerone::TeamYearThankQuery::build_query(variables);
        let response = client
            .http
            .post("https://hackerone.com/graphql")
            .json(&query)
            .send().await
            .unwrap();

        let data =
            response
                .json::<graphql_client::Response<
                    <hackerone::TeamYearThankQuery as GraphQLQuery>::ResponseData,
                >>().await
                .unwrap();
        let can_fetch_team = data
            .data
            .expect("Response data not found")
            .selected_team
            .is_some();

        return Ok(can_fetch_team);
    }

    Ok(true)
}

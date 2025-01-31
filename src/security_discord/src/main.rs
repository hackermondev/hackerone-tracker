extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod breakdown;
mod subscriptions;
mod webhook;

use std::env;

use clap::Parser;
use tokio::sync::mpsc;

#[derive(Default, Debug, Parser)]
#[clap(author = "hackermon", version, about)]
struct Arguments {
    #[arg(short = 'R', long = "redis", env = "REDIS_URL")]
    redis: String,

    #[arg(short = 'W', long = "webhook_url", env = "DISCORD_WEBHOOK_URL")]
    discord_webhook_url: String,
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let args = Arguments::parse();
    debug!("{:#?}", args);

    webhook::set_webhook_url(&args.discord_webhook_url)
        .await
        .expect("invalid webhook");
    env::set_var("REDIS_URL", &args.redis);
    subscriptions::reputation::consume_backlog()
        .await
        .expect("failed to consume reputation backlog");

    let mut tasks = vec![];

    {
        let reputation_task = tokio::task::spawn(async move {
            subscriptions::reputation::reputation_subscription()
                .await
                .expect("reputation subscription failed");
        });

        tasks.push(reputation_task);
    }

    {
        let reports_task = tokio::task::spawn(async move {
            subscriptions::reports::reports_subscription()
                .await
                .expect("reports subscription failed");
        });

        tasks.push(reports_task);
    }

    {
        let informative_reports_task = tokio::task::spawn(async move {
            subscriptions::informative_reports::informative_reports_subscription()
                .await
                .expect("leaderboard reports subscription failed");
        });

        tasks.push(informative_reports_task);
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

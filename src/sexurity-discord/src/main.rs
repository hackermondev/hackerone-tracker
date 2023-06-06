extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod subscriptions;
use clap::Parser;
use reqwest::blocking as reqwest;
use serde::Serialize;
use sexurity_api::redis;
use twilight_model::channel::message::embed::Embed;

#[derive(Default, Debug, Parser)]
#[clap(author = "hackermon", version, about)]
struct Arguments {
    #[arg(short = 'R', long = "redis", env = "REDIS_URL")]
    redis: String,

    #[arg(short = 'W', long = "webhook_url", env = "DISCORD_WEBHOOK_URL")]
    discord_webhook_url: String,
}

#[derive(Serialize)]
struct DiscordMessage {
    embeds: Vec<Embed>,
}

fn main() {
    pretty_env_logger::init();
    info!("hello world");
    let args = Arguments::parse();
    ensure_args_and_return_webhook(&args);
    debug!("{:#?}", args);

    let on_message_data = move |embeds: Vec<Embed>| {
        let message = DiscordMessage { embeds };

        debug!("sending message with embed length {}", message.embeds.len());
        trace!("sending embed: {:#?}", message.embeds);
        let client = reqwest::Client::new();
        client
            .post(args.discord_webhook_url.clone())
            .json(&message)
            .send()
            .unwrap();
    };

    let redis = redis::open(&args.redis).unwrap();
    subscriptions::reputation::consume_backlog(
        redis.get_connection().unwrap(),
        on_message_data.clone(),
    );

    // Subscriptions
    subscriptions::reputation::start_reputation_subscription(
        redis.get_connection().unwrap(),
        on_message_data.clone(),
    );

    subscriptions::reports::start_reports_subscription(
        redis.get_connection().unwrap(),
        on_message_data.clone(),
    );
    keep_alive();
}

fn ensure_args_and_return_webhook(args: &Arguments) {
    let webhook = extract_webhook_info(&args.discord_webhook_url);
    if webhook.is_none() {
        panic!("unable to parse webhook. ensure webhook url is format: https://discord.com/api/webhooks/:id/:token")
    }

    let (webhook_id, webhook_token) = webhook.unwrap();
    let webhook_req = reqwest::get(format!(
        "https://discord.com/api/webhooks/{}/{}",
        webhook_id, webhook_token
    ))
    .unwrap();
    if !webhook_req.status().is_success() {
        panic!("invalid webhook");
    }
}

fn extract_webhook_info(url: &str) -> Option<(u64, &str)> {
    let path_parts: Vec<&str> = url.trim_start_matches("https://").split('/').collect();

    if path_parts.len() >= 4 && path_parts[1] == "api" && path_parts[2] == "webhooks" {
        let webhook_id = path_parts[3].parse::<u64>().ok()?;
        let token = path_parts[4];

        Some((webhook_id, token))
    } else {
        None
    }
}

/// Keep main thread from dying
fn keep_alive() {
    loop {
        let _ = 1 + 1;
    }
}

use std::sync::LazyLock;

use reqwest::Client;
use serde::Serialize;
use tokio::sync::RwLock;
use twilight_model::channel::message::Embed;

#[derive(Serialize)]
struct DiscordMessage {
    embeds: Vec<Embed>,
}

static WEBHOOK_URL: LazyLock<RwLock<String>> = LazyLock::new(|| RwLock::new(String::new()));
static HTTP_REQUEST_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .http2_prior_knowledge()
        .build()
        .expect("failed to build request client")
});

pub async fn deliver_embeds(embeds: Vec<Embed>) -> Result<(), anyhow::Error> {
    let webhook_url = WEBHOOK_URL.read().await;
    let message = DiscordMessage { embeds };
    trace!("sending embed: {:#?}", message.embeds);
    let mut tries = 0;

    loop {
        if tries >= 5 {
            return Err(anyhow::Error::msg("failed to deliver embeds (5 tries)"));
        }

        let client_post_result = HTTP_REQUEST_CLIENT
            .post(&*webhook_url)
            .json(&message)
            .send()
            .await;

        tries += 1;
        if client_post_result.is_ok() {
            break;
        }

        error!("webhook failed {}", client_post_result.err().unwrap());
    }

    Ok(())
}

pub async fn set_webhook_url(webhook_url: &str) -> Result<(), anyhow::Error> {
    let webhook = extract_webhook_info(webhook_url);
    if webhook.is_none() {
        return Err(anyhow::Error::msg("failed to parse webhook, ensure webhook url is format: https://discord.com/api/webhooks/:id/:token"));
    }

    let (webhook_id, webhook_token) = webhook.unwrap();
    let webhook_req = reqwest::get(format!(
        "https://discord.com/api/webhooks/{}/{}",
        webhook_id, webhook_token
    ))
    .await?;

    webhook_req.error_for_status()?;
    let mut static_webhook_url = WEBHOOK_URL.write().await;
    *static_webhook_url = String::from(webhook_url);
    Ok(())
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

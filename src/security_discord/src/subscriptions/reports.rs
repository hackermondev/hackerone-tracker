use futures_util::StreamExt;
use security_api::models;
use security_api::redis;
use twilight_model::channel::message::embed::Embed;
use twilight_util::builder::embed::EmbedBuilder;
use twilight_util::builder::embed::EmbedFieldBuilder;

use crate::webhook;

pub async fn reports_subscription() -> Result<(), anyhow::Error> {
    info!("starting subscription");

    let kv_config = redis::get_config();
    let kv_config = kv_config.url.unwrap();
    let redis = redis::redis::Client::open(kv_config)?;
    let mut pubsub = redis.get_async_pubsub().await?;
    pubsub
        .subscribe(models::redis_keys::REPORTS_QUEUE_PUBSUB)
        .await?;

    let mut stream = pubsub.into_on_message();
    while let Some(message) = stream.next().await {
        let payload: String = message.get_payload().unwrap();
        let decoded: models::ReportsDataQueueItem = serde_json::from_str(&payload).unwrap();
        debug!("reports: recieved message {:#?}", decoded);
        info!(
            "reports: new queue items (id = {}, items = {})",
            decoded.id.clone().unwrap(),
            decoded.diff.len()
        );

        for diff in decoded.diff {
            let embed = build_embed_data(diff);
            if let Some(embed) = embed {
                webhook::deliver_embeds(vec![embed]).await?;
            }
        }
    }

    Ok(())
}

fn build_embed_data(diff: Vec<models::ReportData>) -> Option<Embed> {
    if diff.len() < 2 {
        panic!("invalid diff data");
    }

    let _old = &diff[0];
    let new = &diff[1];

    // tracks disclosed reports
    if new.disclosed {
        // report closed (undisclosed)
        let mut user_field = format!(
            "[**``{}``**]({})",
            new.user_name,
            format!("https://hackerone.com/{}", new.user_name)
        );

        if new.collaboration {
            user_field = format!("{} (+ unknown collaborator)", user_field);
        }

        let title = new
            .title
            .clone()
            .unwrap_or(String::from("(unknown title)"))
            .to_string();
        let summary = new.summary.clone();
        let url = new
            .url
            .clone()
            .unwrap_or(String::from("https://hackerone.com/???"));
        let severity = new
            .severity
            .clone()
            .unwrap_or(String::from("unknown"))
            .to_string();
        let bounty = if new.awarded_amount < 0.0 {
            String::from("hidden")
        } else {
            format!("{} {}", new.awarded_amount, new.currency)
        };

        let mut embed = EmbedBuilder::new()
            .color(models::embed_colors::TRANSPARENT)
            .title(title)
            .url(url)
            .field(EmbedFieldBuilder::new("Reporter", user_field).build());

        if summary.is_some() {
            embed = embed.field(EmbedFieldBuilder::new("Summary", summary.unwrap()).build())
        }

        embed = embed
            .field(
                EmbedFieldBuilder::new("Severity", severity)
                    .inline()
                    .build(),
            )
            .field(
                EmbedFieldBuilder::new("Bounty Award", bounty)
                    .inline()
                    .build(),
            );

        let embed = embed.build();
        return Some(embed);
    }

    None
}

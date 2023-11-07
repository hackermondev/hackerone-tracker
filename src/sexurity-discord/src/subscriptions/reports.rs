use sexurity_api::models;
use sexurity_api::redis::redis::Connection;
use std::thread::{self, JoinHandle};
use twilight_model::channel::message::embed::Embed;
use twilight_util::builder::embed::EmbedFieldBuilder;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};

pub fn start_reports_subscription<E: Fn(Vec<Embed>) + Sync + std::marker::Send + 'static>(
    mut conn: Connection,
    on_message_data: E,
) -> JoinHandle<()> {
    info!("reports: starting subscription");
    thread::spawn(move || {
        let mut pubsub = conn.as_pubsub();
        pubsub
            .subscribe(models::redis_keys::REPORTS_QUEUE_PUBSUB)
            .unwrap();

        loop {
            let msg = pubsub.get_message().unwrap();
            let payload: String = msg.get_payload().unwrap();

            let decoded: models::ReportsDataQueueItem = serde_json::from_str(&payload).unwrap();
            debug!("reports: recieved message {:#?}", decoded);
            info!(
                "reports: new queue items (id = {}, items = {})",
                decoded.id.clone().unwrap(),
                decoded.diff.len()
            );

            for diff in decoded.diff {
                let embed = build_embed_data(diff);
                if embed.is_some() {
                    on_message_data(vec![embed.unwrap()]);
                }
            }
        }
    })
}

fn build_embed_data(diff: Vec<models::ReportData>) -> Option<Embed> {
    if diff.len() < 2 {
        panic!("invalid diff data");
    }

    let _old = &diff[0];
    let new = &diff[1];

    // tracks disclosed reports
    if new.disclosed == true {
        // report closed (undisclosed)
        let mut user_field = format!(
            "[**``{}``**]({})",
            new.user_name,
            format!("https://hackerone.com/{}", new.user_name)
        );

        if new.collaboration {
            user_field = format!("{} (+ unknown collaborator)", user_field);
        }

        let title = format!(
            "{}",
            new.title.clone().unwrap_or(String::from("(unknown title)"))
        );
        let summary = new.summary.clone();
        let url = new
            .url
            .clone()
            .unwrap_or(String::from("https://hackerone.com/???"));
        let severity = format!(
            "{}",
            new.severity.clone().unwrap_or(String::from("unknown"))
        );
        let bounty = if new.awarded_amount < 0.0 {
            String::from("unknown")
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

    // else if old.id.is_none() {
    //     // new report
    //     let mut user_field = format!(
    //         "[**``{}``**]({})",
    //         new.user_name,
    //         format!("https://hackerone.com/{}", new.user_name)
    //     );

    //     if new.collaboration {
    //         user_field = format!("{} (+ unknown collaborator)", user_field);
    //     }

    //     let embed = EmbedBuilder::new()
    //         .color(models::embed_colors::TRANSPARENT)
    //         .title(format!("#{} - Report Closed", new.id.as_ref().unwrap()))
    //         .url(
    //             new.url
    //                 .as_ref()
    //                 .unwrap_or(&"https://hackerone.com/???".to_string()),
    //         )
    //         .field(EmbedFieldBuilder::new("Reporter", user_field).build())
    //         .field(
    //             EmbedFieldBuilder::new(
    //                 "Bounty Award",
    //                 if new.awarded_amount < 0.0 {
    //                     String::from("???")
    //                 } else {
    //                     format!("{} {}", new.awarded_amount, new.currency)
    //                 },
    //             )
    //             .build(),
    //         )
    //         .footer(EmbedFooterBuilder::new("This report is currently private").build())
    //         .build();

    //     return Some(embed);
    // }

    None
}

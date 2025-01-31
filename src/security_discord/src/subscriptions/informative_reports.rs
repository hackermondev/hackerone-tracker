use futures_util::StreamExt;
use security_api::models;
use security_api::redis;
use twilight_model::channel::message::embed::Embed;
use twilight_util::builder::embed::EmbedBuilder;

use crate::webhook;

pub async fn informative_reports_subscription() -> Result<(), anyhow::Error> {
    info!("starting subscription");

    let kv_config = redis::get_config();
    let kv_config = kv_config.url.unwrap();
    let redis = redis::redis::Client::open(kv_config)?;
    let mut pubsub = redis.get_async_pubsub().await?;
    pubsub
        .subscribe(models::redis_keys::USER_INFORMATIVE_REPORTS_QUEUE_PUBSUB)
        .await?;

    let mut stream = pubsub.into_on_message();
    while let Some(message) = stream.next().await {
        let payload: String = message.get_payload().unwrap();
        let item: models::InvalidReportsQueueItem = serde_json::from_str(&payload).unwrap();
        debug!("recieved item {:#?}", item);
        info!("new queue items (changes = {})", item.changes.len());

        let embed = build_embed_data(item.changes, &item.team_handle);
        webhook::deliver_embeds(vec![embed]).await?;
    }

    Ok(())
}

fn build_embed_data(
    changes: Vec<models::UserInvalidReportChange>,
    team_handle: &str,
) -> Embed {
    let program_field = format!("[**``{team_handle}``**](https://hackerone.com/{team_handle})");
    let count_describing_term = if changes.len() == 1 {
        let user_change = &changes[0];
        if user_change.invalid_reports == 1 {
            "a report"
        } else {
            &format!("{} reports", user_change.invalid_reports)
        }
    } else {
        "several reports"
    };

    let users = changes
        .iter()
        .map(|c| {
            let handle = &c.user_name;
            format!("[**``{handle}``**](https://hackerone.com/{handle})")
        })
        .collect::<Vec<String>>();

    let users = if users.len() == 1 {
        &users[0]
    } else if users.len() <= 5 {
        let four_users = &users[..3].join(", ");
        let last_users = &users[4];
        &format!("{four_users} and {last_users}")
    } else {
        let five_users = &users[..3].join(", ");
        let remaining_count = users[3..].len();
        let plural = remaining_count > 1;

        if plural {
            &format!("{five_users} and {remaining_count} other users")
        } else {
            &format!("{five_users} and {remaining_count} other user")
        }
    };

    let text =
        format!("{program_field} closed {count_describing_term} from {users} as Informative");
        
    EmbedBuilder::new()
        .description(text)
        .color(models::embed_colors::INFORMAL).build()
}

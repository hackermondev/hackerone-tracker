use futures_util::StreamExt;
use security_api::models;
use security_api::redis::{self, redis::AsyncCommands};

use twilight_model::channel::message::embed::Embed;
use twilight_model::util::Timestamp;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};

use crate::breakdown::calculate_rep_breakdown;
use crate::webhook;

static MAX_BACKLOG: isize = 1000;

pub async fn consume_backlog() -> Result<(), anyhow::Error> {
    let mut kv = redis::get_connection().get().await?;
    let backlog_raw: Vec<String> = kv
        .zrange(models::redis_keys::REPUTATION_QUEUE_BACKLOG, 0, MAX_BACKLOG)
        .await?;

    debug!("reputation: backlog {:#?}", backlog_raw);
    if !backlog_raw.is_empty() {
        info!(
            "reputation: consuming backlog with {} items",
            backlog_raw.len()
        );
    }

    let mut backlog: Vec<models::RepDataQueueItem> = vec![];
    for backlog_raw_item in backlog_raw {
        let decoded: models::RepDataQueueItem = serde_json::from_str(&backlog_raw_item).unwrap();
        backlog.push(decoded);
    }

    for mut item in backlog {
        item.diff.sort_by_key(|k| k[1].rank);
        for diff in item.diff {
            let handle = diff[0]
                .team_handle
                .clone()
                .unwrap_or_else(|| diff[1].team_handle.clone().unwrap());
            let embed = build_embed_data(diff, &handle, item.include_team_handle).clone();
            if embed.is_some() {
                let mut embed_unwrapped = embed.unwrap();
                embed_unwrapped.timestamp = Some(
                    Timestamp::from_micros(item.created_at.and_utc().timestamp_micros()).unwrap(),
                );

                webhook::deliver_embeds(vec![embed_unwrapped]).await?;
            }
        }
    }

    kv.del::<_, ()>(models::redis_keys::REPUTATION_QUEUE_BACKLOG)
        .await?;
    Ok(())
}

pub async fn reputation_subscription() -> Result<(), anyhow::Error> {
    info!("starting subscription");

    let kv_config = redis::get_config();
    let kv_config = kv_config.url.unwrap();
    let redis = redis::redis::Client::open(kv_config)?;
    let mut pubsub = redis.get_async_pubsub().await?;
    pubsub
        .subscribe(models::redis_keys::REPUTATION_QUEUE_PUBSUB)
        .await?;

    let mut kv = redis::get_connection().get().await?;
    let mut stream = pubsub.into_on_message();

    while let Some(message) = stream.next().await {
        let payload: String = message.get_payload().unwrap();
        let mut decoded: models::RepDataQueueItem = serde_json::from_str(&payload).unwrap();
        debug!("reputation: recieved message {:#?}", decoded);
        info!(
            "reputation: new queue items (id = {}, items = {})",
            decoded.id.clone().unwrap(),
            decoded.diff.len()
        );

        // try to sort by rep
        decoded.diff.sort_by_key(|k| k[1].rank);
        for diff in decoded.diff {
            let handle = diff[0]
                .team_handle
                .clone()
                .unwrap_or_else(|| diff[1].team_handle.clone().unwrap());
            let embed = build_embed_data(diff, &handle, decoded.include_team_handle);
            if let Some(embed) = embed {
                webhook::deliver_embeds(vec![embed]).await?;
            }
        }

        kv.del::<_, ()>(models::redis_keys::REPUTATION_QUEUE_BACKLOG)
            .await?;
    }

    Ok(())
}

fn build_embed_data(
    diff: Vec<models::RepData>,
    handle: &str,
    include_team_handle: bool,
) -> Option<Embed> {
    if diff.len() < 2 {
        panic!("invalid diff data");
    }

    let old = &diff[0];
    let new = &diff[1];

    if old.reputation == -1 {
        // new user added to leaderboard
        let mut text = format!(
            "[**``{}``**]({}) was added to [**``{}``**]({}) with **{} reputation** (rank: #{})",
            new.user_name,
            format!("https://hackerone.com/{}", new.user_name),
            handle,
            format!("https://hackerone.com/{}", handle),
            new.reputation,
            new.rank
        );

        if new.rank == -1 {
            text = format!("[**``{}``**]({}) was added to [**``{}``**]({}) with **{} reputation** (rank: >100)", new.user_name, format!("https://hackerone.com/{}", new.user_name), handle, format!("https://hackerone.com/{}", handle), new.reputation);
        }

        let mut embed = EmbedBuilder::new()
            .description(text)
            .color(models::embed_colors::POSTIVE);

        if new.rank >= 50 {
            embed = embed.color(models::embed_colors::MAJOR);
        }

        return Some(embed.build());
    } else if new.reputation == -1 {
        // user removed from leaderboard
        let text = format!(
            "[**``{}``**]({}) was removed from [**``{}``**]({})",
            old.user_name,
            format!("https://hackerone.com/{}", old.user_name),
            handle,
            format!("https://hackerone.com/{}", handle),
        );

        let embed = EmbedBuilder::new()
            .description(text)
            .color(models::embed_colors::NEGATIVE)
            .build();
        return Some(embed);
    } else if new.reputation > old.reputation {
        // reputation gain
        let change = new.reputation - old.reputation;
        let breakdown = calculate_rep_breakdown(change as i32);
        let mut text = format!(
            "[**``{}``**]({}) gained **+{} reputation** and now has **{} reputation**",
            new.user_name,
            format!("https://hackerone.com/{}", new.user_name),
            change,
            new.reputation,
        );

        if include_team_handle {
            text += &format!(
                " in [**``{}``**]({})",
                handle,
                format!("https://hackerone.com/{}", handle)
            );
        }

        let mut embed_builder = EmbedBuilder::new()
            .description(text)
            .color(models::embed_colors::POSTIVE);

        if change >= 50 {
            embed_builder = embed_builder.color(models::embed_colors::MAJOR);
        }

        {
            let mut footer = String::from("");
            if new.rank < old.rank {
                footer += &format!("#{} -> #{} (+{})", old.rank, new.rank, old.rank - new.rank);
            }

            let breakdown = breakdown.to_string();
            if !breakdown.is_empty() {
                if !footer.is_empty() {
                    footer += " • "
                };

                footer += &breakdown;
            }

            if !footer.is_empty() {
                embed_builder = embed_builder.footer(EmbedFooterBuilder::new(footer));
            }
        }

        let embed = embed_builder.build();
        return Some(embed);
    } else if old.reputation > new.reputation {
        // reputation lost
        let change = new.reputation - old.reputation;
        let breakdown = calculate_rep_breakdown(change as i32);
        let mut text = format!(
            "[**``{}``**]({}) lost **{} reputation** and now has **{} reputation**",
            new.user_name,
            format!("https://hackerone.com/{}", new.user_name),
            new.reputation - old.reputation,
            new.reputation,
        );

        if include_team_handle {
            text += &format!(
                " in [**``{}``**]({})",
                handle,
                format!("https://hackerone.com/{}", handle)
            );
        }

        let mut embed_builder = EmbedBuilder::new()
            .description(text)
            .color(models::embed_colors::NEGATIVE);

        {
            let mut footer = String::from("");
            if new.rank < old.rank {
                footer += &format!("#{} -> #{} (-{})", old.rank, new.rank, new.rank - old.rank);
            }

            let breakdown = breakdown.to_string();
            if !breakdown.is_empty() {
                if !footer.is_empty() {
                    footer += "| "
                };

                footer += &breakdown;
            }

            if !footer.is_empty() {
                embed_builder = embed_builder.footer(EmbedFooterBuilder::new(footer));
            }
        }

        let embed = embed_builder.build();
        return Some(embed);
    }

    None
}

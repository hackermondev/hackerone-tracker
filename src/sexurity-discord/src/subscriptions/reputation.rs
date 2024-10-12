use sexurity_api::models;
use sexurity_api::redis::redis::cmd;
use sexurity_api::redis::redis::Connection;
use std::thread;
use std::thread::JoinHandle;
use twilight_model::channel::message::embed::Embed;
use twilight_model::util::Timestamp;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};

use crate::breakdown::calculate_rep_breakdown;
static MAX_BACKLOG: usize = 100;
pub fn consume_backlog<E: Fn(Vec<Embed>)>(mut conn: Connection, on_message_data: E) {
    let backlog_raw = cmd("ZRANGE")
        .arg(models::redis_keys::REPUTATION_QUEUE_BACKLOG)
        .arg(0)
        .arg(MAX_BACKLOG)
        .query::<Vec<String>>(&mut conn)
        .unwrap();

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
        // try to sort by rep
        item.diff.sort_by_key(|k| k[1].rank);
        for diff in item.diff {
            let handle = diff[0]
                .team_handle
                .clone()
                .unwrap_or_else(|| diff[1].team_handle.clone().unwrap());
            let embed = build_embed_data(diff, &handle, item.include_team_handle).clone();
            if embed.is_some() {
                let mut embed_unwrapped = embed.unwrap();
                embed_unwrapped.timestamp =
                    Some(Timestamp::from_micros(item.created_at.and_utc().timestamp_micros()).unwrap());

                on_message_data(vec![embed_unwrapped]);
            }
        }
    }

    cmd("DEL")
        .arg(models::redis_keys::REPUTATION_QUEUE_BACKLOG)
        .query::<i32>(&mut conn)
        .unwrap();
}

pub fn start_reputation_subscription<E: Fn(Vec<Embed>) + Sync + std::marker::Send + 'static>(
    mut conn: Connection,
    mut conn2: Connection,
    on_message_data: E,
) -> JoinHandle<()> {
    info!("reputation: starting subscription");
    thread::spawn(move || {
        let mut pubsub = conn.as_pubsub();
        pubsub
            .subscribe(models::redis_keys::REPUTATION_QUEUE_PUBSUB)
            .unwrap();

        // let test_embed = EmbedBuilder::new().description("hey").build();
        // on_message_data(vec![test_embed]);
        loop {
            let msg = pubsub.get_message().unwrap();
            let payload: String = msg.get_payload().unwrap();

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
                if embed.is_some() {
                    on_message_data(vec![embed.unwrap()]);
                }
            }

            cmd("DEL")
                .arg(models::redis_keys::REPUTATION_QUEUE_BACKLOG)
                .query::<i32>(&mut conn2)
                .unwrap();
        }
    })
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

        let embed = EmbedBuilder::new()
            .description(text)
            .color(models::embed_colors::POSTIVE)
            .build();
        return Some(embed);
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

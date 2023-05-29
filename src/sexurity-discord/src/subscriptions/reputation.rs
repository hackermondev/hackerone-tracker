use sexurity_api::models;
use sexurity_api::redis::redis::cmd;
use sexurity_api::redis::redis::Connection;
use std::thread;
use twilight_model::channel::message::embed::Embed;
use twilight_model::util::Timestamp;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};

static MAX_BACKLOG: usize = 100;
pub fn consume_backlog<E: Fn(Vec<Embed>)>(mut conn: Connection, on_message_data: E) {
    let backlog_raw = cmd("ZRANGE")
        .arg(models::redis_keys::REPUTATION_QUEUE_BACKLOG)
        .arg(0)
        .arg(MAX_BACKLOG)
        .query::<Vec<String>>(&mut conn)
        .unwrap();

    debug!("reputation: backlog {:#?}", backlog_raw);
    if backlog_raw.len() > 0 {
        info!("reputation: consuming backlog with {} items", backlog_raw.len());
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
            let embed = build_embed_data(diff, &item.team_handle);
            if embed.is_some() {
                let mut embed_unwrapped = embed.unwrap();
                embed_unwrapped.timestamp =
                    Some(Timestamp::from_micros(item.created_at.timestamp_micros()).unwrap());

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
    on_message_data: E,
) {
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
            info!("reputation: new queue items (id = {}, items = {})", decoded.id.clone().unwrap(), decoded.diff.len());

            // try to sort by rep
            decoded.diff.sort_by_key(|k| k[1].rank);
            for diff in decoded.diff {
                let embed = build_embed_data(diff, &decoded.team_handle);
                if embed.is_some() {
                    on_message_data(vec![embed.unwrap()]);
                }
            }
        }
    });
}

fn build_embed_data(diff: Vec<models::RepData>, handle: &str) -> Option<Embed> {
    if diff.len() < 2 {
        panic!("invalid diff data");
    }

    let old = &diff[0];
    let new = &diff[1];

    if old.rank == -1 {
        // new user added to leaderboard
        let text = format!(
            "[**``{}``**]({}) was added to [**``{}``**]({}) with **{} reputation** (rank: #{})",
            new.user_name,
            format!("https://hackerone.com/{}", new.user_name),
            handle,
            format!("https://hackerone.com/{}", handle),
            new.reputation,
            new.rank
        );

        let embed = EmbedBuilder::new()
            .description(text)
            .color(models::embed_colors::POSTIVE)
            .build();
        return Some(embed);
    } else if new.rank == -1 {
        // user removed from leaderboard
        let text = format!(
            "[**``{}``**]({}) was removed from top 100",
            old.user_name,
            format!("https://hackerone.com/{}", old.user_name),
        );

        let embed = EmbedBuilder::new()
            .description(text)
            .color(models::embed_colors::NEGATIVE)
            .build();
        return Some(embed);
    } else if new.reputation > old.reputation {
        // reputation gain
        let text = format!(
            "[**``{}``**]({}) gained **+{} reputation** and now has **{} reputation**",
            new.user_name,
            format!("https://hackerone.com/{}", new.user_name),
            new.reputation - old.reputation,
            new.reputation,
        );

        let mut embed_builder = EmbedBuilder::new()
            .description(text)
            .color(models::embed_colors::POSTIVE);
        if new.rank < old.rank {
            let footer = format!("#{} -> #{} (+{})", old.rank, new.rank, old.rank - new.rank);
            embed_builder = embed_builder.footer(EmbedFooterBuilder::new(footer));
        }

        let embed = embed_builder.build();
        return Some(embed);
    } else if old.reputation > new.reputation {
        // reputation lost
        let text = format!(
            "[**``{}``**]({}) lost **{} reputation** and now has **{} reputation**",
            new.user_name,
            format!("https://hackerone.com/{}", new.user_name),
            new.reputation - old.reputation,
            new.reputation,
        );

        let mut embed_builder = EmbedBuilder::new()
            .description(text)
            .color(models::embed_colors::NEGATIVE);
        if new.rank > old.rank {
            let footer = format!("#{} -> #{} (-{})", old.rank, new.rank, new.rank - old.rank);
            embed_builder = embed_builder.footer(EmbedFooterBuilder::new(footer));
        }

        let embed = embed_builder.build();
        return Some(embed);
    } else if old.rank != new.rank {
        // rank change
        let text = format!(
            "[**``{}``**]({}) rank changed. #{} -> #{}",
            new.user_name,
            format!("https://hackerone.com/{}", new.user_name),
            old.rank,
            new.rank
        );

        let embed = EmbedBuilder::new()
            .description(text)
            .color(models::embed_colors::POSTIVE)
            .build();

        return Some(embed);
    }

    None
}

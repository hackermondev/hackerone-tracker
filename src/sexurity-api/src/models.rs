use chrono::{DateTime, Utc};
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RepData {
    pub reputation: i64,
    pub rank: i64,
    pub user_name: String,
    pub user_profile_image_url: String,
    pub user_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RepDataQueueItem {
    pub id: Option<String>,
    pub team_handle: String,
    pub diff: Vec<Vec<RepData>>,

    #[serde(with = "my_date_format")]
    pub created_at: DateTime<Utc>,
}

impl RepDataQueueItem {
    pub fn create_id(&mut self) {
        // TODO: get rid of nanoid, write a unique id func
        let id = nanoid!();
        self.id = Some(id);
    }
}

pub mod embed_colors {
    pub const NEGATIVE: u32 = 16711680;
    pub const POSTIVE: u32 = 5222492;
}

pub mod redis_keys {
    pub const REPUTATION_QUEUE_BACKLOG: &str = "reputation_queue";
    pub const REPUTATION_QUEUE_PUBSUB: &str = "reputation_poll_queue";
    pub const REPUTATION_QUEUE_LAST_RUN_TIME: &str = "reputation_poll_last_run_time";
    pub const REPUTATION_QUEUE_LAST_DATA: &str = "reputation_poll_last_data";
}

mod my_date_format {
    use chrono::{DateTime, TimeZone, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    const FORMAT: &'static str = "%Y-%m-%d %H:%M:%S";

    // The signature of a serialize_with function must follow the pattern:
    //
    //    fn serialize<S>(&T, S) -> Result<S::Ok, S::Error>
    //    where
    //        S: Serializer
    //
    // although it may also be generic over the input types T.
    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = format!("{}", date.format(FORMAT));
        serializer.serialize_str(&s)
    }

    // The signature of a deserialize_with function must follow the pattern:
    //
    //    fn deserialize<'de, D>(D) -> Result<T, D::Error>
    //    where
    //        D: Deserializer<'de>
    //
    // although it may also be generic over the output types T.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Utc.datetime_from_str(&s, FORMAT)
            .map_err(serde::de::Error::custom)
    }
}

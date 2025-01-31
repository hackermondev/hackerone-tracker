use chrono::NaiveDateTime;
use nanoid::nanoid;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Deserialize, Serialize, Clone)]
pub struct RepData {
    pub reputation: i64,
    pub rank: i64,
    pub user_name: String,
    pub user_profile_image_url: String,
    pub user_id: String,
    pub team_handle: Option<String>,
}

#[derive(Default, Debug, Deserialize, Serialize)]
pub struct RepDataQueueItem {
    pub id: Option<String>,
    pub diff: Vec<Vec<RepData>>,
    pub include_team_handle: bool,

    #[serde(with = "my_date_format")]
    pub created_at: NaiveDateTime,
}

impl RepDataQueueItem {
    pub fn create_id(&mut self) {
        let id = nanoid!();
        self.id = Some(id);
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ReportData {
    pub user_name: String,
    pub user_id: String,

    pub currency: String,
    pub awarded_amount: f64,

    pub id: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,

    pub summary: Option<String>,
    pub severity: Option<String>,
    pub collaboration: bool,
    pub disclosed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReportsDataQueueItem {
    pub id: Option<String>,
    pub team_handle: Option<String>,
    pub diff: Vec<Vec<ReportData>>,

    #[serde(with = "my_date_format")]
    pub created_at: NaiveDateTime,
}

impl ReportsDataQueueItem {
    pub fn create_id(&mut self) {
        let id = nanoid!();
        self.id = Some(id);
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UserThanksData {
    pub user_id: String,
    pub user_name: String,
    pub team_handle: String,
    pub resolved_report_count: i64,
    pub invalid_report_count: i64,
    pub total_report_count: i64,
    pub reputation: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UserInvalidReportChange {
    pub user_id: String,
    pub user_name: String,
    pub invalid_reports: i64,
    pub team_handle: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InvalidReportsQueueItem {
    pub changes: Vec<UserInvalidReportChange>,
    pub team_handle: String,
}

pub mod embed_colors {
    pub const NEGATIVE: u32 = 16711680;
    pub const POSTIVE: u32 = 5222492;
    pub const MAJOR: u32 = 16567356;
    pub const INFORMAL: u32 = 8882052;
    pub const TRANSPARENT: u32 = 2829617;
}

pub mod redis_keys {
    pub const REPUTATION_QUEUE_BACKLOG: &str = "reputation_queue";
    pub const REPUTATION_QUEUE_PUBSUB: &str = "reputation_poll_queue";
    pub const REPUTATION_QUEUE_LAST_DATA: &str = "reputation_poll_last_data";

    pub const REPORTS_QUEUE_PUBSUB: &str = "reports_poll_queue";
    pub const REPORTS_POLL_LAST_RUN_TIME: &str = "reports_poll_last_run_time";
    pub const REPORTS_POLL_LAST_DATA: &str = "reports_poll_last_data";
    
    pub const PROGRAMS: &str = "programs";

    pub const USER_INFORMATIVE_REPORTS_QUEUE_PUBSUB: &str = "user_informative_reports_queue";
    pub const USER_THANKS_DATA_POLL_LAST_DATA: &str = "user_program_thanks_data_poll_last_data";
}

mod my_date_format {
    use chrono::NaiveDateTime;
    use serde::{self, Deserialize, Deserializer, Serializer};

    const FORMAT: &str = "%Y-%m-%d %H:%M:%S";

    // The signature of a serialize_with function must follow the pattern:
    //
    //    fn serialize<S>(&T, S) -> Result<S::Ok, S::Error>
    //    where
    //        S: Serializer
    //
    // although it may also be generic over the input types T.
    pub fn serialize<S>(date: &NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error>
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
    pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        NaiveDateTime::parse_from_str(&s, FORMAT)
            .map_err(serde::de::Error::custom)
    }
}

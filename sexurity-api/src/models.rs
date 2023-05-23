use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RepData {
    pub reputation: i64,
    pub rank: i64,
    pub user_name: String,
    pub user_profile_image_url: String,
    pub user_id: String,
}
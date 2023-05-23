use sexurity_api::redis::Client;
use sexurity_api::hackerone::{HackerOneClient};
pub mod reputation;

pub struct PollConfiguration {
    pub hackerone: HackerOneClient,
    pub team_handle: String,
    pub redis_client: Client,
}
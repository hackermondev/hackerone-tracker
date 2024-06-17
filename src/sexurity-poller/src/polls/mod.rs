use sexurity_api::hackerone::HackerOneClient;
use sexurity_api::redis::redis::Client;
pub mod programs;
pub mod reports;
pub mod reputation;

#[derive(Clone)]
pub struct PollConfiguration {
    pub hackerone: HackerOneClient,
    /// If this is `None`, all programs are tracked
    pub team_handle: Option<String>,
    pub redis_client: Client,
}

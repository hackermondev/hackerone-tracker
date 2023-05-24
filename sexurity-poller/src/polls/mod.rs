use sexurity_api::hackerone::HackerOneClient;
use sexurity_api::redis::redis::Client;
pub mod reputation;

#[derive(Clone)]
pub struct PollConfiguration {
    pub hackerone: HackerOneClient,
    pub team_handle: String,
    pub redis_client: Client,
}

use security_api::hackerone::HackerOneClient;
pub mod programs;
pub mod reports;
pub mod reputation;
pub mod informative_reports;

#[derive(Clone)]
pub struct PollConfiguration {
    pub hackerone: HackerOneClient,
    pub team_handle: Option<String>,
}

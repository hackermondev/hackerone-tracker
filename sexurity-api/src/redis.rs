pub use redis::Client;
extern crate redis;

pub fn open(url: &str) -> Result<Client, Box<dyn std::error::Error>> {
    let client = redis::Client::open(url)?;
    Ok(client)
}
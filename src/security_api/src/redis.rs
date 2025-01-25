use std::sync::LazyLock;

pub use deadpool_redis::redis;
use deadpool_redis::{self as deadpool, redis::AsyncCommands, Connection};

static GLOBAL_REDIS_POOL: LazyLock<deadpool::Pool> = LazyLock::new(|| {
    let config = get_config();
    config.create_pool(Some(deadpool::Runtime::Tokio1)).unwrap()
});

pub fn get_connection() -> deadpool::Pool {
    GLOBAL_REDIS_POOL.clone()
}

pub fn get_config() -> deadpool::Config {
    let url = std::env::var("REDIS_URL");
    let url = url.expect("Redis connection URI required");
    deadpool::Config::from_url(url)
}

pub async fn save_vec_to_set<'a, V: serde::Deserialize<'a> + serde::Serialize>(
    name: &str,
    data: Vec<V>,
    overwrite: bool,
    redis: &mut Connection,
) -> Result<(), anyhow::Error> {
    if overwrite {
        redis.del::<_, ()>(&name).await?;
    }

    for i in data {
        let value_name = serde_json::to_string(&i)?;
        redis.sadd::<_, _, ()>(&name, value_name).await?;
    }

    Ok(())
}

pub async fn load_set_to_vec(
    name: &str,
    redis: &mut Connection,
) -> Result<Vec<String>, anyhow::Error> {
    let set_members = redis.smembers::<_, Vec<String>>(&name).await?;
    let mut result = Vec::new();
    for mut value in set_members {
        if value.starts_with('"') {
            value = value[1..value.len() - 1].to_string();
        }

        result.push(value);
    }

    Ok(result)
}

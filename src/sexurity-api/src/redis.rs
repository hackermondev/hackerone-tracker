pub use redis;

pub fn open(url: &str) -> Result<redis::Client, Box<dyn std::error::Error>> {
    let client = redis::Client::open(url)?;
    Ok(client)
}

pub fn save_vec_to_set<'a, V: serde::Deserialize<'a> + serde::Serialize>(
    name: String,
    data: Vec<V>,
    overwrite: bool,
    mut conn: &mut redis::Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    if overwrite {
        redis::cmd("DEL").arg(&name).query(&mut conn)?;
    }

    for i in data {
        let value_name = serde_json::to_string(&i)?;
        redis::cmd("SADD")
            .arg(&name)
            .arg(value_name)
            .query(&mut conn)?;
    }

    Ok(())
}

pub fn load_set_to_vec(
    name: String,
    mut conn: &mut redis::Connection,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let set_values: Vec<String> = redis::cmd("SMEMBERS").arg(&name).query(&mut conn)?;

    let mut result = Vec::new();
    for value in set_values {
        result.push(value);
    }

    Ok(result)
}

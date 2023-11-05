
use anyhow::Result;

pub fn get(key: &str) -> Result<String> {
    let key = key.to_uppercase();
    Ok(std::env::var(key)?)
}
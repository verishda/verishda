
use anyhow::Result;

pub fn get(key: &str) -> Result<String> {
    let key = (String::new() + "SPIN_CONFIG_" + key).to_uppercase();
    Ok(std::env::var(key)?)
}

use std::{collections::HashMap, iter::Map};

use anyhow::{Result, anyhow};
use dotenv::*;


pub trait Config: Send + Sync{
    fn get(&self, key: &str) -> Result<String>;
    fn clone_box_dyn(&self) -> Box<dyn Config>;
}

impl Clone for Box<dyn Config> {
    fn clone(&self) -> Self {
        self.clone_box_dyn()
    }
}

#[derive(Clone)]
pub struct CompositeConfig {
    main: Box<dyn Config>,
    fallback: Box<dyn Config>,
}

impl CompositeConfig {
    pub fn from_configs(main: Box<dyn Config>, fallback: Box<dyn Config>) -> CompositeConfig {
        CompositeConfig{ main, fallback }
    }
}

impl Config for CompositeConfig {
    fn get(&self, key: &str) -> Result<String> {
        self.main
        .get(key)
        .or_else(|_e| self.fallback.get(key))
    }

    fn clone_box_dyn(&self) -> Box<dyn Config> {
        Box::new(CompositeConfig {
            main: self.main.clone_box_dyn(),
            fallback: self.fallback.clone_box_dyn()
        })
    }
}



#[derive(Clone)]
pub struct EnvConfig;

impl EnvConfig {
    pub fn from_env() -> EnvConfig {
        match dotenv() {
            Ok(path) => {
                let path = path.to_string_lossy();
                println!("additional environment variables loaded from {path}");
            }
            Err(e) => {
                println!("error while attempting to load .env file: {e}");
            }
        }

        EnvConfig
    }
}

const PUBLIC_ISSUER_URL: &str = "https://lemur-5.cloud-iam.com/auth/realms/verishda"; 
const PUBLIC_CLIENT_ID: & str = "verishda-windows";
const PUBLIC_API_BASE_URL: &str = "https://verishda.shuttleapp.rs";
//const PUBLIC_API_BASE_URL: &str = "http://127.0.0.1:3000";

pub fn default_config() -> impl Config {
    let default_values = [
        ("ISSUER_URL", PUBLIC_ISSUER_URL),
        ("CLIENT_ID", PUBLIC_CLIENT_ID),
        ("API_BASE_URL", PUBLIC_API_BASE_URL),
    ];
    let mut default_config = HashMap::<String,String>::new();
    for (k,v) in default_values {
        default_config.insert(k.to_string(), v.to_string());
    }
    HashMapConfig::from(default_config)
}

impl Config for EnvConfig{
    fn get(&self, key: &str) -> Result<String> {
        std::env::var(key).map_err(|_| anyhow!("no such environment variable {key}"))
    }
    fn clone_box_dyn(&self) -> Box<dyn Config> {
        Box::new(self.clone())
    }
}

struct HashMapConfig {
    map: HashMap<String,String>
}

impl HashMapConfig {
    pub fn new() -> HashMapConfig{
        Self::from(HashMap::new())
    }
}

impl From<HashMap<String,String>> for HashMapConfig
{
    fn from(map: HashMap<String,String>) -> HashMapConfig {
        Self {map}
    }
}

impl Config for HashMapConfig {
    fn get(&self, key: &str) -> Result<String> {
        self.map
        .get(key)
        .map(String::clone)
        .ok_or_else(||anyhow!("key '{key}' not found"))
    }

    fn clone_box_dyn(&self) -> Box<dyn Config> {
        Box::new(HashMapConfig{
            map: self.map.clone()
        })
    }
}

#[test]
fn test_default_composite_config() {

    let mut hashmap_config = HashMapConfig::new();
    hashmap_config.map.insert("CLIENT_ID".into(), "test-client".into());

    let config = CompositeConfig::from_configs(Box::new(hashmap_config), Box::new(default_config()));
    
    assert_eq!(config.get("ISSUER_URL").unwrap(), PUBLIC_ISSUER_URL);
    assert_eq!(config.get("CLIENT_ID").unwrap(), "test-client");
}

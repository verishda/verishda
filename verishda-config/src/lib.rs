
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

pub fn default_config() -> impl Config {
    let default_values = [
        ("ISSUER_URL", "https://lemur-5.cloud-iam.com/auth/realms/verishda"),
        ("CLIENT_ID", "verishda-windows"),
    ];
    let mut default_config = HashMap::<String,String>::new();
    for (k,v) in default_values {
        default_config.insert(k.to_string(), v.to_string());
    }
    HashMapConfig::new(default_config)
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
    pub fn new(map: HashMap<String,String>) -> HashMapConfig {
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
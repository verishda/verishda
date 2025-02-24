
use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use dotenv::*;


/// The `Config` trait allows access to process-wide configuration.
/// Configuration items can be pulled from a mix of various sources:
/// * `.env` files
/// * environment variables
/// * special system specific configuration stores like the Windows
///   registry
/// * custom configuration stores
/// 
/// Configuration stored here works like a (possibly typed) key/value
/// store. A `Config` implementation must declare which configuration
/// properties is supports. See [Config::supported_keys]
/// 
pub trait Config: Send + Sync{
    /// return specific keys names that this `Config` implementation 
    /// supports. Note that this is in addition to what [Config::supports_any_key]
    /// provides. 
    /// The default implementation returns the empty set.
    fn supported_settable_keys(&self) -> HashSet<&str>{
        HashSet::new()
    }
    /// return whether this `Config` supports reading and writing any
    /// arbitrary key. 
    /// The default implementation returns always `false`.
    fn supports_setting_any_key(&self) -> bool{
        false
    }

    fn get(&self, key: &str) -> Result<String>;
    fn set(&mut self, _key: &str, _value: &str) -> Result<()> {
        panic!("set operation not implemented, check if setting config properties is supported via supported_settable_keys() and supports_setting_any_key() methods")
    }

    /// Create polymorphic copy of concrete `Config` trait object
    fn clone_box_dyn(&self) -> Box<dyn Config>;

    fn get_as_bool_or(&self, key: &str, default: bool) -> bool {
        self.get(key).ok().map(|s|s=="true").unwrap_or(default)
    }
    fn set_as_bool(&mut self, key: &str, value: bool) -> Result<()> {
        self.set(key, &value.to_string())
    }

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

    fn is_settable(config: &Box<dyn Config>, key: &str) -> bool {
        config.supports_setting_any_key()
        || config.supported_settable_keys().contains(key)
    }

    fn settable_config(&mut self, key: &str) -> Option<&mut Box<dyn Config>> {
        if Self::is_settable(&self.main, key) {
            Some(&mut self.main)
        } else if Self::is_settable(&self.fallback, key) {
            Some(&mut self.fallback)
        } else {
            None
        }
    }
}

impl Config for CompositeConfig {

    fn supported_settable_keys(&self) -> HashSet<&str> {
        let hs = self.main.supported_settable_keys();
        hs.union(&self.fallback.supported_settable_keys())
        .map(|s|*s)
        .collect()
    }

    fn supports_setting_any_key(&self) -> bool {
        return self.main.supports_setting_any_key()
        ||  self.fallback.supports_setting_any_key()
    }

    fn get(&self, key: &str) -> Result<String> {
        self.main
        .get(key)
        .or_else(|_e| self.fallback.get(key))
    }


    fn set(&mut self, key: &str, value: &str) -> Result<()>{
        if let Some(settable_config) = self.settable_config(key) {
            settable_config.set(key, value)
        } else {
            Err(anyhow!("key '{key}' can not be set in config"))
        }
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
const PUBLIC_API_BASE_URL: &str = "https://verishda-lkej.shuttle.app";
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

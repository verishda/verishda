

use std::{sync::{Arc, RwLock}, collections::HashMap};

use crate::store::KeyByteValueStore;

use anyhow::anyhow;

/// A `KeyByteValueStore` implementation a HashMap.
#[derive(Default, Clone)]
pub struct MemoryStore {
    store: Arc<RwLock<HashMap<String,Vec<u8>>>>
}
impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KeyByteValueStore for MemoryStore
where 
{
    fn get(&self, key: &str) -> Result<Vec<u8>, anyhow::Error> {
        match self.store.read().unwrap().get(key) {
            Some(v) => Ok(v.clone()),
            None => Err(anyhow!("no entry found")),
        }
    }

    fn set(&mut self, key: &str, value: Vec<u8>) -> Result<(), anyhow::Error> {
        self.store.write().unwrap().insert(key.to_string(), value);
        Ok(())
    }

    fn delete(&mut self, key: &str) -> Result<(), anyhow::Error> {
        self.store.write().unwrap().remove(key);
        Ok(())
    }
}
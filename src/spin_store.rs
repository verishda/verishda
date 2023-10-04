

use crate::store::KeyByteValueStore;

use anyhow::anyhow;

/// A `KeyByteValueStore` implementation using spin's key value store.
pub struct SpinStore {
    store: spin_sdk::key_value::Store
}
impl SpinStore {
    pub fn new(store: spin_sdk::key_value::Store) -> Self {
        Self { store }
    }
}

impl KeyByteValueStore for SpinStore
where 
{
    fn get(&self, key: &str) -> Result<Vec<u8>, anyhow::Error> {
        match self.store.get(key) {
            Ok(v) => Ok(v),
            Err(e) => Err(anyhow!(e)),
        }
    }

    fn set(&mut self, key: &str, value: Vec<u8>) -> Result<(), anyhow::Error> {
        match self.store.set(key, value) {
            Ok(()) => Ok(()),
            Err(e) => Err(anyhow!(e))
        }
    }

    fn delete(&mut self, key: &str) -> Result<(), anyhow::Error> {
        match self.store.delete(key) {
            Ok(()) => Ok(()),
            Err(e) => Err(anyhow!(e))
        }
    }
}
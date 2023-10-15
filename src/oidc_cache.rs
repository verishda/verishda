use std::ops::Add;
use std::time::{SystemTime, UNIX_EPOCH, Duration};

use log::{trace, error};
use openidconnect::core::CoreJsonWebKeySet;

use openidconnect::core::CoreProviderMetadata;
use serde::Deserialize;
use serde::Serialize;


use crate::store::KeyByteValueStore;

use crate::store::Cache;

const CACHE_EXPIRY_DURATION: std::time::Duration = std::time::Duration::from_secs(300);

/// specific `Cache` implementation storing OIDC metadata
pub struct MetadataCache<S>
where S: KeyByteValueStore
{
    store: S
}

impl <S> MetadataCache<S> 
where S: KeyByteValueStore {
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

#[derive(Serialize, Deserialize)]
struct CacheItem
{
    metadata: CoreProviderMetadata,
    keys: CoreJsonWebKeySet,
    expires_at_secs: u64,
}


impl <S> Cache<str,CoreProviderMetadata> for MetadataCache<S> 
where 
    S: KeyByteValueStore
    {
    
    fn try_get_or_else(&mut self, key: &str, f: impl FnOnce(&str)->Result<CoreProviderMetadata, anyhow::Error>) -> Result<CoreProviderMetadata, anyhow::Error>
    {
        trace!("retrieving entry from spin KVS");
        let raw_result = self.store.get(key);
        let now = SystemTime::now();
        if let Ok(raw) = raw_result {
            if let Ok(cache_item) = serde_json::from_slice::<CacheItem>(&raw) {
                let exp = UNIX_EPOCH.add(Duration::from_secs(cache_item.expires_at_secs));
                if now < exp {
                    trace!("cache hit")
                    // we hit the cache and return the content
                    let meta = cache_item.metadata.set_jwks(cache_item.keys);
                    return Ok(meta)
                } 
            } else {
                error!("broken entry {key} found in spin KVS that cannot be deserialized, deleting...");
                // delete errornous entry that we cannot deserialize
                self.store.delete(key)?;
            }            
        }

        match f(key) {
            Ok(v) => {
                let exp = now.add(CACHE_EXPIRY_DURATION);
                if let Ok(expires_at) = exp.duration_since(UNIX_EPOCH){
                    let item = CacheItem{
                        expires_at_secs: expires_at.as_secs(),
                        keys: v.jwks().clone(),
                        metadata: v.clone()
                    };
                    self.store.set(&key, serde_json::to_vec(&item).unwrap())
                    .expect("storing metadata failed");
                }
                Ok(v)
            },
            Err(e) => Err(e),
        }
    }

}
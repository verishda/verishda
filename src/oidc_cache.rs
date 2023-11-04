use std::ops::Add;
use std::time::{SystemTime, UNIX_EPOCH, Duration};

use log::{trace, error};
use openidconnect::core::CoreJsonWebKeySet;

use openidconnect::core::CoreProviderMetadata;
use serde::Deserialize;
use serde::Serialize;

use anyhow::anyhow;


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
    
    fn get(&self, key: &str) -> Option<CoreProviderMetadata>{
        trace!("retrieving entry from spin KVS");
        let raw_result = self.store.get(key);
        let now = SystemTime::now();
        if let Ok(raw) = raw_result {
            if let Ok(cache_item) = serde_json::from_slice::<CacheItem>(&raw) {
                let exp = UNIX_EPOCH.add(Duration::from_secs(cache_item.expires_at_secs));
                if now < exp {
                    trace!("return cached metadata instead of retrieving it from source");
                    // we hit the cache and return the content
                    let meta = cache_item.metadata.set_jwks(cache_item.keys);
                    return Some(meta)
                } 
            }            
        }

        return None;

    }
    fn set(&mut self, key: &str, v: CoreProviderMetadata) -> anyhow::Result<()> {
        let now = SystemTime::now();
        let exp = now.add(CACHE_EXPIRY_DURATION);
        if let Ok(expires_at) = exp.duration_since(UNIX_EPOCH){
            let item = CacheItem{
                expires_at_secs: expires_at.as_secs(),
                keys: v.jwks().clone(),
                metadata: v.clone()
            };
            self.store.set(&key, serde_json::to_vec(&item).unwrap())
            ?;
            Ok(())
        } else {
            Err(anyhow!(""))
        }

    }
}
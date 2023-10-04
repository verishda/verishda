use log::{trace, error};
use openidconnect::{core::CoreProviderMetadata, JsonWebKeySet};

use crate::store::KeyByteValueStore;

use crate::store::Cache;

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

impl <S> Cache<str,CoreProviderMetadata> for MetadataCache<S> 
where 
    S: KeyByteValueStore
    {
    
    fn try_get_or_else(&mut self, key: &str, f: impl FnOnce(&str)->Result<CoreProviderMetadata, anyhow::Error>) -> Result<CoreProviderMetadata, anyhow::Error>
    {

        let keys_key = String::from(key) + "_keys";
        trace!("retrieving entry from spin KVS");
        let raw_result = (self.store.get(key), self.store.get(&keys_key));
        if let (Ok(raw), Ok(raw_keys)) = raw_result {
            if let (Ok(meta), Ok(jwks)) = 
                (serde_json::from_slice::<CoreProviderMetadata>(&raw), 
                serde_json::from_slice::<JsonWebKeySet<_,_,_,_>>(&raw_keys)) {
                let meta = meta.set_jwks(jwks);
                return Ok(meta)
            } else {
                error!("broken entry {key} found in spin KVS that cannot be deserialized, deleting...");
                // delete errornous entry that we cannot deserialize
                self.store.delete(key)?;
            }            
        }

        match f(key) {
            Ok(v) => {
                self.store.set(&keys_key, serde_json::to_vec(&v.jwks()).unwrap())
                .expect("storing metadata key set failed");
                self.store.set(&key, serde_json::to_vec(&v).unwrap())
                .expect("storing metadata failed");
                Ok(v)
            },
            Err(e) => Err(e),
        }
    }

}
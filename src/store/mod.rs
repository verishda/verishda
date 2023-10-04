
/// A very simple definition of a cache. 
/// 
/// Implementations may provide special behaviour, and may use more complex structures like
/// a `KeyByteValueStore` (e.g. as a storage layer).
pub trait Cache<K,V> 
where K: ?Sized
{
    fn try_get_or_else(&mut self, key: &str, f: impl FnOnce(&str)->Result<V, anyhow::Error>) -> Result<V,anyhow::Error>;
}

/// Define an abstract key value store using `str` values as keys and `Vec<u8>` as 
/// values. 
/// 
/// The goal is to decouple KVS implementations (e.g. spin) from the places
/// where it's used (e.g. OIDC metadata caching logic).
pub trait KeyByteValueStore {
    fn get(&self, key: &str) -> Result<Vec<u8>, anyhow::Error>;
    fn set(&mut self, key: &str, value: Vec<u8>) -> Result<(), anyhow::Error>;
    fn delete(&mut self, key: &str) -> Result<(), anyhow::Error>;
}
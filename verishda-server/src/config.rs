
use anyhow::Result;


pub trait Config: Send + Sync{
    fn get(&self, key: &str) -> Result<String>;
    fn clone_box_dyn(&self) -> Box<dyn Config>;
}

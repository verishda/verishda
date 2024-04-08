#[cfg(target_os = "windows")]
mod windows;

pub use windows::startup;
pub use windows::open_url;

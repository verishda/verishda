#[cfg(target_os = "windows")]
mod windows;

#[cfg(unix)]
mod unix;

pub fn open_url(url: &str) -> anyhow::Result<()>{
    Ok(webbrowser::open(url)?)
}

#[cfg(windows)]
pub use windows::startup;

#[cfg(unix)]
pub use unix::startup;


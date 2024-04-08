
use std::process::Command;
use anyhow::Result;

pub fn open_url(url: &str) -> Result<()> {

    println!("open_url: {}", url);
    Command::new("cmd.exe")
        .args(["/C", "start", "", &url.replace("&", "^&")])
        .spawn()?;
    Ok(())
}
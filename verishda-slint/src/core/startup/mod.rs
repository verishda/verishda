use anyhow::*;
use verishda_config::Config;
use std::{collections::HashSet, str::FromStr};

#[cfg(target_os="windows")]
mod windows;
#[cfg(target_os="macos")]
mod macos;

pub trait StartupBehaviour {
    fn run_on_startup_supported() -> bool;

    fn set_run_on_startup_enabled(run_on_startup: bool) -> Result<()>;

    fn get_run_on_startup_enabled() -> Result<bool>;

}

#[cfg(target_os="windows")]
type PlatformStartupBehaviour = windows::WindowsStartupBehaviour;
#[cfg(target_os="macos")]
type PlatformStartupBehaviour = macos::MacOSStartupBehaviour;

#[derive(Clone)]
pub struct StartupConfig;

const RUN_ON_STARTUP_SUPPORTED: &str = "RUN_ON_STARTUP_SUPPORTED";
const RUN_ON_STARTUP: &str = "RUN_ON_STARTUP";

impl Config for StartupConfig
{
    fn supported_settable_keys(&self) -> HashSet<&str> {
        HashSet::from([RUN_ON_STARTUP])
    }

    fn get(&self, key: &str) -> anyhow::Result<String> {
        match key {
            RUN_ON_STARTUP_SUPPORTED => Ok(PlatformStartupBehaviour::run_on_startup_supported().to_string()),
            RUN_ON_STARTUP => Ok(PlatformStartupBehaviour::get_run_on_startup_enabled()?.to_string()),
            _ => return Err(anyhow!("unknown key"))
        }
    }

    fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            RUN_ON_STARTUP => {
                let v: bool = value.parse()?;
                PlatformStartupBehaviour::set_run_on_startup_enabled(v)
            },
            _ => Err(anyhow!("unsupported key {key}"))
        }
    }

    fn clone_box_dyn(&self) -> Box<dyn Config> {
        let s = (*self).clone();
        Box::new(s)
    }
}
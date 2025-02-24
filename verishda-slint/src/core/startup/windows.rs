
use anyhow::{anyhow,Result};

use windows_registry::*;

pub(crate) struct WindowsStartupBehaviour;

const APP_KEY_NAME: &str = "com.pachler.verishda";
const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

impl WindowsStartupBehaviour {
    fn exec_path() -> Result<String> {
        let exec_path = std::env::current_exe()
        .map_err(|_|anyhow!("executable path unavailable"))?;
        let exec_path = exec_path
        .to_str()
        .ok_or(anyhow!("cannot convert PathBuf to String"))?
        ;
        let exec_path = format!("\"{exec_path}\"");
        Ok(exec_path.into())
    }
}

impl super::StartupBehaviour for WindowsStartupBehaviour {
    
    fn set_run_on_startup_enabled(run_on_startup: bool) -> Result<()> {
        let run_key = CURRENT_USER.create(RUN_KEY_PATH)?;
        if run_on_startup {
            let exec_path = Self::exec_path()?;
            run_key.set_hstring(APP_KEY_NAME, &exec_path.into())?;
        } else {
            run_key.remove_value(APP_KEY_NAME)?;
        }

        Ok(())
    }
    
    fn get_run_on_startup_enabled() -> anyhow::Result<bool> {
        let run_key = CURRENT_USER.open(RUN_KEY_PATH)?;

        let run_key_value = match run_key.get_hstring(APP_KEY_NAME) {
            Ok(v) => v,
            Err(_e) => return Ok(false),
        };

        return Ok(run_key_value == Self::exec_path()?);
    }
    
    fn run_on_startup_supported() -> bool {
        true
    }

    
}
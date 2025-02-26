use anyhow::*;

pub struct MacOSStartupBehaviour;

impl super::StartupBehaviour for MacOSStartupBehaviour {
    fn run_on_startup_supported() -> bool {
        false
    }

    fn set_run_on_startup_enabled(_run_on_startup: bool) -> Result<()> {
        unimplemented!()
    }

    fn get_run_on_startup_enabled() -> Result<bool> {
        Ok(false)
    }
}
#[cfg(target_os="macos")]
mod macos;

pub(crate) fn init() {
    #[cfg(target_os="macos")]
    macos::init();
}
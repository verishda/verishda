#![windows_subsystem = "windows"]

use std::sync::Arc;
use tokio::sync::Mutex;

use slint::Weak;
use tokio::runtime::Handle;

slint::include_modules!();

mod platform;
mod core;

use core::AppCore;

const PUBLIC_ISSUER_URL: &str = "https://lemur-5.cloud-iam.com/auth/realms/werischda";
const PUBLIC_CLIENT_ID: &str = "verishda-windows";

fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    runtime.block_on(async {
        tokio::task::spawn_blocking(ui_main)
        .await
        .unwrap();
    });
}

fn ui_main() {

    platform::startup(AppCore::uri_scheme());

    let app_core = Arc::new(Mutex::new(AppCore::new()));

    let main_window = MainWindow::new().unwrap();
    let main_window_weak = main_window.as_weak();
    let app_core_clone = app_core.clone();
    main_window.global::<AppUI>().on_login_triggered(move||{
        start_login(app_core_clone.clone(), main_window_weak.clone());
    });

    main_window.show().unwrap();

    let main_window_weak = main_window.as_weak();
    start_fetch_provider_metadata(main_window_weak.clone(), app_core);

    // NOT: will need to change to slint::run_event_loop_until_quit() when we have a systray icon
    slint::run_event_loop().unwrap();
}

fn start_fetch_provider_metadata(main_window: Weak<MainWindow>, app_core: Arc<Mutex<AppCore>>) {
    main_window.unwrap().global::<AppUI>().set_state(MainWindowState::FetchingProviderMetadata);
    Handle::current().spawn(async move {
        let mut app_core = app_core.lock().await;
        match app_core.init_provider(PUBLIC_ISSUER_URL, PUBLIC_CLIENT_ID).await {
            Ok(_) => {
                main_window.upgrade_in_event_loop(|main_window|{
                    let app_ui = main_window.global::<AppUI>();
                    app_ui.set_state(MainWindowState::ShowingWelcomeView);
                }).unwrap();
            },
            Err(_) =>
                panic!("Failed to fetch provider metadata")
        };
    });

}

fn start_login(app_core: Arc<Mutex<AppCore>>, main_window_weak: Weak<MainWindow>) {
    let auth_url = app_core.blocking_lock().authorization_url();
    if let Err(e) = platform::open_url(&auth_url) {
        eprintln!("Failed to open URL: {}", e);
    }


    // MISSING: actual login via OIDC & Browser
    //main_window_weak.unwrap().global::<AppUI>().set_state(MainWindowState::ShowingSitePresenceView);



}

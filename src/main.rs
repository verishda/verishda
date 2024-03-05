use std::sync::{Arc, Mutex};

use slint::Weak;
use tokio::runtime::Handle;
use openidconnect::core::CoreProviderMetadata; // Add this import statement
use openidconnect::IssuerUrl; // Add this import statement
use openidconnect::reqwest::async_http_client;

slint::include_modules!();

mod platform;

const PUBLIC_ISSUER_URL: &str = "https://lemur-5.cloud-iam.com/auth/realms/werischda";

struct AppCore {
    metadata: Option<CoreProviderMetadata>,
    credentials: Option<()>,    // MISSING: actual credentials

}

fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    runtime.block_on(async {
        tokio::task::spawn_blocking(ui_main)
        .await
        .unwrap();
    });
}

fn ui_main() {

    platform::startup();
    let main_window = MainWindow::new().unwrap();
    let main_window_weak = main_window.as_weak();
    main_window.global::<AppUI>().on_login_triggered(move ||{
        // MISSING: actual login via OIDC & Browser
        main_window_weak.unwrap().global::<AppUI>().set_state(MainWindowState::ShowingSitePresenceView);
    });

    main_window.show().unwrap();

    let main_window_weak = main_window.as_weak();

    let app_core = Arc::new(Mutex::new(AppCore {
        metadata: None,
        credentials: None,
    }));

    start_fetch_provider_metadata(main_window_weak, app_core);

    // NOT: will need to change to slint::run_event_loop_until_quit() when we have a systray icon
    slint::run_event_loop().unwrap();
}

fn start_fetch_provider_metadata(main_window: Weak<MainWindow>, app_core: Arc<Mutex<AppCore>>) {
    main_window.unwrap().global::<AppUI>().set_state(MainWindowState::FetchingProviderMetadata);
    Handle::current().spawn(async move {
        if let Ok(provider_metadata) = CoreProviderMetadata::discover_async(
            IssuerUrl::new(PUBLIC_ISSUER_URL.to_string()).unwrap(),
            async_http_client,
        ).await {
            app_core.lock().unwrap().metadata = Some(provider_metadata);

            main_window.upgrade_in_event_loop(|main_window|{
                let app_ui = main_window.global::<AppUI>();
                app_ui.set_state(MainWindowState::ShowingWelcomeView);
            }).unwrap();
        } else {
            panic!("Failed to fetch provider metadata")
        }
    });

}

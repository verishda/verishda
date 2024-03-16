//#![windows_subsystem = "windows"]

use std::sync::Arc;
use tokio::sync::Mutex;

use slint::{MapModel, Model, ModelRc, SharedString, VecModel, Weak};
use tokio::runtime::Handle;
use crate::client::types::Site;

slint::include_modules!();

mod platform;
mod core;
mod client;

use core::AppCore;

const PUBLIC_ISSUER_URL: &str = "https://lemur-5.cloud-iam.com/auth/realms/werischda";
const PUBLIC_CLIENT_ID: &str = "verishda-windows";

fn main() {

    let args: Vec<String> = std::env::args().collect();

    let runtime = tokio::runtime::Runtime::new().unwrap();

    // check if we are being called to handle a redirect
    if let (Some(param), Some(url)) = (args.get(1), args.get(2)) {
        if param != AppCore::redirect_url_param() {
            eprintln!("Unknown parameter: {}", param);
            std::process::exit(1);
        } else {
            runtime.block_on(async {
                match AppCore::handle_login_redirect(url).await {
                    Ok(()) => std::process::exit(0),
                    Err(e) => {
                        eprintln!("Failed to handle login redirect: {}", e);
                        std::process::exit(2);
                    }
                }
                });
        }
    } else {
        runtime.block_on(async {
            tokio::task::spawn_blocking(ui_main)
            .await
            .unwrap();
        });    
    }
}

fn ui_main() {

    platform::startup(AppCore::uri_scheme(), AppCore::redirect_url_param());

    let app_core = AppCore::new();

    let main_window = MainWindow::new().unwrap();
    let main_window_weak = main_window.as_weak();
    let app_core_clone = app_core.clone();
    let app_ui = main_window.global::<AppUI>();
    app_ui.on_login_triggered(move||{
        start_login(app_core_clone.clone(), main_window_weak.clone());
    });

    // wire site_names to sites property, mapping names. This is so that
    // the site names can be shown in the ComboBox, which only accepts [string],
    // not [SiteModel]
    app_ui.set_sites(ModelRc::new(VecModel::default()));
    let site_names = app_ui.get_sites().map(|site| site.name.clone());
    app_ui.set_site_names(ModelRc::new(site_names));

    let main_window_weak = main_window.as_weak();
    let app_core_clone = app_core.clone();
    app_ui.on_login_cancelled(move||{
        cancel_login(app_core_clone.clone(), main_window_weak.clone());
    });

    let main_window_weak = main_window.as_weak();
    app_core.blocking_lock().on_core_event(move |event|{
        main_window_weak.upgrade_in_event_loop(|main_window|{
            let app_ui = main_window.global::<AppUI>();

//            let site_names = MapModel::new(app_ui.get_sites(), |site| site.name.clone());
            match event {
                core::CoreEvent::SitesUpdated(sites) => {
                    let sites_model = app_ui.get_sites();
                    let sites_model = sites_model.as_any()
                    .downcast_ref::<VecModel<SiteModel>>()
                    .expect("we set VecModel<> earlier");
                    
                    let sites_vec: Vec<SiteModel> = sites.iter()
                    .map(|site| site.into())
                    .collect();
                    
                    sites_model.set_vec(sites_vec);
                },
                _ => {}
            }
        }).unwrap();        
    });

    main_window.show().unwrap();

    let main_window_weak = main_window.as_weak();
    let app_core_clone = app_core.clone();
    start_fetch_provider_metadata(main_window_weak.clone(), app_core_clone);

    // NOT: will need to change to slint::run_event_loop_until_quit() when we have a systray icon
    slint::run_event_loop().unwrap();

    app_core.blocking_lock().quit();

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
    main_window_weak.unwrap().global::<AppUI>().set_state(MainWindowState::ShowingWaitingForLoginView);

    let mw = main_window_weak.clone();
    let auth_url = if let Ok(auth_url) = AppCore::start_login(app_core.clone(), move |logged_in|{
        mw.upgrade_in_event_loop(move |main_window: MainWindow|{
            let app_ui = main_window.global::<AppUI>();
            if logged_in {
                app_ui.set_state(MainWindowState::ShowingSitePresenceView);
            } else {
                app_ui.set_state(MainWindowState::ShowingWelcomeView);
            }
        }).expect("cannot upgrade main window weak reference to strong reference in event loop");
    
    }) {
        auth_url
    } else {
        eprintln!("Failed to start login");
        return
    };
    if let Err(e) = platform::open_url(&auth_url.to_string()) {
        eprintln!("Failed to open URL: {}", e);
    }
}

fn cancel_login(app_core: Arc<Mutex<AppCore>>, main_window: Weak<MainWindow>){
    app_core.blocking_lock().cancel_login();
    main_window.upgrade_in_event_loop(|main_window|{
        let app_ui = main_window.global::<AppUI>();
        app_ui.set_state(MainWindowState::ShowingWelcomeView);
    }).unwrap();
}

impl Into<SiteModel> for &Site {
    fn into(self) -> SiteModel {
        SiteModel {
            id: self.id.clone().into(),
            name: self.name.clone().into(),
        }
    }
}
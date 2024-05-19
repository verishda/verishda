#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;

use chrono::{Datelike, Days};
use verishda_dto::types::{Presence, Site};
use std::{collections::HashSet, env, sync::Arc};
use tokio::sync::Mutex;

use core::Announcement;
use slint::{Model, ModelRc, VecModel, Weak};
use tokio::runtime::Handle;

slint::include_modules!();

mod core;
mod platform;

use core::AppCore;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    redirect_url: Option<String>,
}

fn main() {
    let args = Args::parse();

    simple_logger::SimpleLogger::new().env().init().unwrap();
    log::info!("Starting up Verishda");

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _g = runtime.enter();

    // check if we are being called to handle a redirect
    if let Some(url) = &args.redirect_url {
        runtime.block_on(async {
            match AppCore::handle_login_redirect(url).await {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("Failed to handle login redirect: {}", e);
                    std::process::exit(2);
                }
            }
        });
    } else {
        /*runtime.block_on(async {
            tokio::task::spawn_blocking(ui_main)
            .await
            .unwrap();
        });
        */
        ui_main();
    }
}

fn ui_main() {
    platform::startup(AppCore::uri_scheme(), AppCore::redirect_url_param());

    let app_core = AppCore::new();

    let main_window = MainWindow::new().unwrap();
    let main_window_weak = main_window.as_weak();
    let app_core_clone = app_core.clone();
    let app_ui = main_window.global::<AppUI>();
    app_ui.on_using_invitation_code_requested(move |code| {
        log::info!("requsted using code {code}");
    });
    app_ui.on_login_triggered(move || {
        start_login(app_core_clone.clone(), main_window_weak.clone());
    });

    // wire site_names to sites property, mapping names. This is so that
    // the site names can be shown in the ComboBox, which only accepts [string],
    // not [SiteModel]
    app_ui.set_sites(ModelRc::new(VecModel::default()));
    let site_names = app_ui.get_sites().map(|site| site.name.clone());
    app_ui.set_site_names(ModelRc::new(site_names));

    app_ui.set_persons(ModelRc::new(VecModel::default()));

    let main_window_weak = main_window.as_weak();
    let app_core_clone = app_core.clone();
    app_ui.on_login_cancelled(move || {
        cancel_login(app_core_clone.clone(), main_window_weak.clone());
    });

    let app_core_clone = app_core.clone();
    app_ui.on_site_selected(move |site_id| {
        site_selected(app_core_clone.clone(), &site_id);
    });

    let app_core_clone = app_core.clone();
    app_ui.on_refresh_requested(move || {
        refresh_requested(app_core_clone.clone());
    });

    let app_core_clone = app_core.clone();
    app_ui.on_announcement_change_requested(move |site_id, person, day_index| {
        log::info!("Announcement change requested: {site_id}, {person:?}, {day_index}");
        announce(app_core_clone.clone(), site_id.to_string(), person);
    });

    let main_window_weak = main_window.as_weak();
    app_core.blocking_lock().on_core_event(move |event| {
        main_window_weak
            .upgrade_in_event_loop(|main_window| {
                let app_ui = main_window.global::<AppUI>();

                match event {
                    core::CoreEvent::SitesUpdated(sites) => {
                        let sites_model = app_ui.get_sites();
                        let sites_model = sites_model
                            .as_any()
                            .downcast_ref::<VecModel<SiteModel>>()
                            .expect("we set VecModel<> earlier");

                        let sites_vec: Vec<SiteModel> =
                            sites.iter().map(|site| site.into()).collect();

                        sites_model.set_vec(sites_vec);
                    }
                    core::CoreEvent::PresencesChanged(presences) => {
                        let persons_model = app_ui.get_persons();
                        let persons_model = persons_model
                            .as_any()
                            .downcast_ref::<VecModel<PersonModel>>()
                            .expect("we set VecModel<> earlier");

                        let persons_vec: Vec<PersonModel> =
                            presences.iter().map(to_person_model).collect();

                        persons_model.set_vec(persons_vec);

                        let current_day =
                            chrono::Local::now().weekday().num_days_from_monday() as i32;
                        app_ui.set_current_day_index(current_day)
                    }
                }
            })
            .unwrap();
    });

    let main_window_weak = std::sync::Mutex::new(main_window.as_weak());

    #[cfg(windows)]
    let _tray = init_systray(
        move || {
            main_window_weak
                .lock()
                .unwrap()
                .upgrade_in_event_loop(|main_window| {
                    let _ = main_window.show();
                })
                .unwrap();
        },
        || {
            let _ = slint::quit_event_loop();
        },
    );

    main_window.show().unwrap();

    let main_window_weak = main_window.as_weak();
    let app_core_clone = app_core.clone();
    start_fetch_provider_metadata(main_window_weak.clone(), app_core_clone);

    // NOT: will need to change to slint::run_event_loop_until_quit() when we have a systray icon
    slint::run_event_loop_until_quit().unwrap();

    app_core.blocking_lock().quit();
}

#[cfg(windows)]
fn init_systray<FO, FQ>(open_handler: FO, quit_handler: FQ) -> tray_item::TrayItem
where
    FO: Fn() + Send + Sync + 'static,
    FQ: Fn() + Send + Sync + 'static,
{
    use tray_item::*;

    let mut tray = TrayItem::new("Verishda", IconSource::Resource("tray-default")).unwrap();

    tray.add_label("Verishda").unwrap();

    tray.add_menu_item("Open", open_handler).unwrap();

    tray.add_menu_item("Quit", quit_handler).unwrap();

    tray
}

fn start_fetch_provider_metadata(main_window: Weak<MainWindow>, app_core: Arc<Mutex<AppCore>>) {
    main_window
        .unwrap()
        .global::<AppUI>()
        .set_state(MainWindowState::Startup);
    Handle::current().spawn(async move {
        let mut app_core = app_core.lock().await;
        match app_core.init().await {
            Ok(_) => {
                main_window
                    .upgrade_in_event_loop(|main_window| {
                        let app_ui = main_window.global::<AppUI>();
                        app_ui.set_state(MainWindowState::ShowingWelcomeView);
                    })
                    .unwrap();
            }
            Err(_) => panic!("Failed to fetch provider metadata"),
        };
    });
}

fn start_login(app_core: Arc<Mutex<AppCore>>, main_window_weak: Weak<MainWindow>) {
    main_window_weak
        .unwrap()
        .global::<AppUI>()
        .set_state(MainWindowState::ShowingWaitingForLoginView);

    let mw = main_window_weak.clone();
    let auth_url = if let Ok(auth_url) = AppCore::start_login(app_core.clone(), move |logged_in| {
        mw.upgrade_in_event_loop(move |main_window: MainWindow| {
            log::info!("Logged in: {logged_in}");
            let app_ui = main_window.global::<AppUI>();
            if logged_in {
                app_ui.set_state(MainWindowState::ShowingSitePresenceView);
            } else {
                app_ui.set_state(MainWindowState::ShowingWelcomeView);
            }
        })
        .expect("cannot upgrade main window weak reference to strong reference in event loop");
    }) {
        auth_url
    } else {
        eprintln!("Failed to start login");
        return;
    };
    if let Err(e) = platform::open_url(&auth_url.to_string()) {
        eprintln!("Failed to open URL: {}", e);
    }
}

fn cancel_login(app_core: Arc<Mutex<AppCore>>, main_window: Weak<MainWindow>) {
    app_core.blocking_lock().cancel_login();
    main_window
        .upgrade_in_event_loop(|main_window| {
            let app_ui = main_window.global::<AppUI>();
            app_ui.set_state(MainWindowState::ShowingWelcomeView);
        })
        .unwrap();
}

fn site_selected(app_core: Arc<Mutex<AppCore>>, site_id: &str) {
    log::info!("Site selected: {site_id}");
    app_core.blocking_lock().set_site(site_id);
}

fn refresh_requested(app_core: Arc<Mutex<AppCore>>) {
    log::info!("Refresh requested");
    app_core.blocking_lock().refresh();
}

fn announce(app_core: Arc<Mutex<AppCore>>, site_id: String, person: PersonModel) {
    let person_announcements = person.announcements.clone();
    log::debug!("Announcement made on site {site_id:?} as {person:?} with announcements {person_announcements:?}");
    let announcements = person
        .announcements
        .iter()
        .map(AnnouncementModel::into)
        .collect();
    log::debug!("converted as announcements {announcements:?}");
    app_core.blocking_lock().announce(site_id, announcements);
}

impl Into<SiteModel> for &Site {
    fn into(self) -> SiteModel {
        SiteModel {
            id: self.id.clone().into(),
            name: self.name.clone().into(),
        }
    }
}

impl Into<Announcement> for AnnouncementModel {
    fn into(self) -> Announcement {
        match self {
            AnnouncementModel::NotAnnounced => Announcement::NotAnnounced,
            AnnouncementModel::PresenceAnnounced => Announcement::PresenceAnnounced,
            AnnouncementModel::RecurringPresenceAnnounced => Announcement::WeeklyPresenceAnnounced,
        }
    }
}

const ANNOUNCED_DAYS_AHEAD: u32 = 7;

fn to_person_model(presence: &Presence) -> PersonModel {
    let dates = presence
        .announced_dates
        .iter()
        .collect::<HashSet<_>>();

    let now_date = chrono::Utc::now().naive_utc().date();

    let announcements = (0..ANNOUNCED_DAYS_AHEAD)
        .into_iter()
        .map(|n| {
            let is_announced = now_date
                .checked_add_days(Days::new(n as u64))
                .is_some_and(|date| dates.contains(&date));
            if is_announced {
                AnnouncementModel::PresenceAnnounced
            } else {
                AnnouncementModel::NotAnnounced
            }
        })
        .collect::<Vec<_>>();

    log::debug!("announcements in person: {announcements:?}");

    PersonModel {
        name: presence.logged_as_name.clone().into(),
        is_present: presence.currently_present,
        // TODO! implement this
        announcements: ModelRc::new(VecModel::from(announcements)),
        is_self: presence.is_self,
    }
}

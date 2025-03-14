#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;

use chrono::{Datelike, Days};
use core::{verishda_dto::types::{Presence, PresenceAnnouncementKind, Site}, Settings};
use std::{collections::HashMap, env};

use core::{Announcement, AppCoreRef, CoreEvent, PersonFilter};
use slint::{Model, ModelRc, VecModel, Weak};
use verishda_config::{default_config, CompositeConfig, Config, EnvConfig};

slint::include_modules!();

mod core;

use core::AppCore;

const BUILD_DATE: &str = env!("BUILD_DATE");
const CARGO_PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    redirect_url: Option<String>,
}

fn main() {
    let args = Args::parse();

    #[cfg(not(target_os = "macos"))]
    {
        
        simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .env()
        .init()
        .unwrap();
    }
    #[cfg(target_os = "macos")]
    {
        const SUBSYSTEM: &str = "com.pachler.verishda-slint";
        println!("IMPORTANT: Verishda logging uses os_log. To see log messages, use the 'Console' application and filter by sybsystem '{SUBSYSTEM}'");
        oslog::OsLogger::new(SUBSYSTEM)
        .level_filter(log::LevelFilter::Debug)
        .init()
        .unwrap();
    }

    log::info!("Starting up Verishda");

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _g = runtime.enter();

    ui_main();
}


fn to_settings_model<C>(config: &C) -> SettingsModel 
where C: Config
{
    SettingsModel {
        run_on_startup: config.get_as_bool_or("RUN_ON_STARTUP", false),
        run_on_startup_supported: config.get_as_bool_or("RUN_ON_STARTUP_SUPPORTED", false),
        software_version: format!("{CARGO_PKG_VERSION} - {BUILD_DATE}").into(),
        ..Default::default()
    }
}

impl Into<Settings> for SettingsModel {
    fn into(self) -> Settings {
        Settings::new(self.run_on_startup)
    }
}

fn ui_main() {
    let inital_config = mk_config();

    let settings_model: SettingsModel = to_settings_model(&inital_config);
    let app_core = AppCore::new(Box::new(inital_config));

    let main_window = MainWindow::new().unwrap();
    let main_window_weak = main_window.as_weak();
    let app_core_clone = app_core.clone();
    let app_ui = main_window.global::<AppUI>();
    app_ui.on_login_triggered(move || {
        start_login(&app_core_clone, main_window_weak.clone());
    });
    let main_window_weak = main_window.as_weak();
    let app_core_clone = app_core.clone();
    app_ui.on_logout_triggered(move || {
        start_logout(&app_core_clone, main_window_weak.clone());
    });

    // wire site_names to sites property, mapping names. This is so that
    // the site names can be shown in the ComboBox, which only accepts [string],
    // not [SiteModel]
    app_ui.set_sites(ModelRc::new(VecModel::default()));
    let site_names = app_ui.get_sites().map(|site| site.name.clone());
    app_ui.set_site_names(ModelRc::new(site_names));

    app_ui.set_persons(ModelRc::new(VecModel::default()));

    app_ui.set_settings(settings_model);

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
    app_ui.on_change_favorite_requested(move |user_id, favorite| {
        change_favorite_requested(app_core_clone.clone(), &user_id, favorite)
    });
    let app_core_clone = app_core.clone();
    app_ui.on_filter_set(move |term, favorites_only| {
        let term = term.trim();
        let term = if !term.is_empty() {
            Some(term.to_owned())
        } else {
            None
        };
        log::info!("setting filter to {term:?}, {favorites_only}");
        set_filter(app_core_clone.clone(), term, favorites_only)
    });
    
    let app_core_clone = app_core.clone();
    app_ui.on_announcement_change_requested(move |site_id, person, day_index| {
        log::info!("Announcement change requested: {site_id}, {person:?}, {day_index}");
        announce(app_core_clone.clone(), site_id.to_string(), person);
    });

    let app_core_clone = app_core.clone();
    app_ui.on_apply_settings_requested(move |settings_model|{
        app_core_clone.apply_settings(settings_model.into())
    });

    let main_window_weak = main_window.as_weak();
    app_core.on_core_event(move |event| {
        log::debug!("core event received: {event:?}");
        main_window_weak
            .upgrade_in_event_loop(|main_window| {
                let app_ui = main_window.global::<AppUI>();

                process_event(app_ui, event);
             })
            .unwrap();
    });

    main_window.show().unwrap();

    slint::run_event_loop().unwrap();

    app_core.quit();
}

fn process_event(app_ui: AppUI<'_>, event: CoreEvent) {
    match event {
        core::CoreEvent::InitializationFinished => 
            app_ui.set_state(MainWindowState::ShowingWelcomeView),
        core::CoreEvent::InitializationFailed =>
            panic!("Failed to fetch provider metadata"),
        core::CoreEvent::LoggingIn => 
            app_ui.set_state(MainWindowState::ShowingWaitingForLoginView),
        core::CoreEvent::LogginSuccessful => 
            app_ui.set_state(MainWindowState::ShowingSitePresenceView),
        core::CoreEvent::LoggedOut => 
            app_ui.set_state(MainWindowState::ShowingWelcomeView),
        core::CoreEvent::SitesUpdated{sites, selected_index} => {
            let sites_model = app_ui.get_sites();
            let sites_model = sites_model
                .as_any()
                .downcast_ref::<VecModel<SiteModel>>()
                .expect("we set VecModel<> earlier");

            let sites_vec: Vec<SiteModel> =
                sites.iter().map(|site| site.into()).collect();
        
            sites_model.set_vec(sites_vec);
            app_ui.set_selected_site_index(selected_index.map(|i|i as i32).unwrap_or(-1))
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
        core::CoreEvent::Terminating => ()  // no special handling for termination for now
    }
}

fn mk_config() -> impl Config {
    let cfg = CompositeConfig::from_configs(
        Box::new(EnvConfig::from_env()), 
        Box::new(default_config())
    );
    let cfg = CompositeConfig::from_configs(
        Box::new(core::startup::StartupConfig{}), 
        Box::new(cfg)
    );
    cfg
}


fn start_login(app_core: &AppCoreRef, _main_window_weak: Weak<MainWindow>) {
    app_core.start_login();
}

fn start_logout(app_core: &AppCoreRef, _main_window_weak: Weak<MainWindow>) {
    app_core.start_logout();
}

fn cancel_login(app_core: AppCoreRef, main_window: Weak<MainWindow>) {
    app_core.cancel_login();
    main_window
        .upgrade_in_event_loop(|main_window| {
            let app_ui = main_window.global::<AppUI>();
            app_ui.set_state(MainWindowState::ShowingWelcomeView);
        })
        .unwrap();
}

fn site_selected(app_core: AppCoreRef, site_id: &str) {
    log::info!("Site selected: {site_id}");
    app_core.set_site(site_id);
}

fn refresh_requested(app_core: AppCoreRef) {
    log::info!("Refresh requested");
    app_core.refresh();
}

fn change_favorite_requested(app_core: AppCoreRef, user_id: &str, favorite: bool) {
    log::info!("favorite state change requested for user {user_id}: {favorite}");
    app_core.change_favorite(user_id, favorite);
}

fn set_filter(app_core: AppCoreRef, term: Option<String>, favorites_only: bool) {
    log::info!("favorit only filter set: {favorites_only}");
    app_core.filter(PersonFilter{term, favorites_only})
}

fn announce(app_core: AppCoreRef, site_id: String, person: PersonModel) {
    let person_announcements = person.announcements.clone().iter().collect::<Vec<_>>();
    log::debug!("Announcement made on site {site_id:?} as {person:?} with announcements {person_announcements:?}");
    let announcements = person
        .announcements
        .iter()
        .map(AnnouncementModel::into)
        .collect();
    log::debug!("converted as announcements {announcements:?}");
    app_core.announce(site_id, announcements);
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
    let now_date = chrono::Local::now().date_naive();

    let dates = presence
        .announcements
        .iter()
        .filter_map(|a|{
            let date;
            match a.kind {
                PresenceAnnouncementKind::RecurringAnnouncement => {
                    let day_offset = a.date.signed_duration_since(now_date).num_days();
                    if day_offset >= 0 {
                        date = a.date;
                    } else {
                        date = if let Some(adjusted_date) = now_date.checked_add_days(Days::new(((day_offset % 7)+7) as u64)) {
                            adjusted_date
                        } else {
                            return None
                        }
                    }
                }
                PresenceAnnouncementKind::SingularAnnouncement =>
                    date = a.date,
            }
            Some((date, a.kind))
        })
        .collect::<HashMap<_,_>>();

    let announcements = (0..ANNOUNCED_DAYS_AHEAD)
        .into_iter()
        .map(|n| {
            let announcement = now_date
                .checked_add_days(Days::new(n as u64))
                .and_then(|date| dates.get(&date));
            match announcement {
                Some(kind) => match kind {
                    &PresenceAnnouncementKind::SingularAnnouncement => AnnouncementModel::PresenceAnnounced,
                    &PresenceAnnouncementKind::RecurringAnnouncement => AnnouncementModel::RecurringPresenceAnnounced,
                },
                None =>           
                    AnnouncementModel::NotAnnounced
            }
        })
        .collect::<Vec<_>>();

    log::debug!("announcements in person: {announcements:?}");

    PersonModel {
        name: presence.logged_as_name.clone().into(),
        user_id: presence.user_id.clone().into(),
        is_present: presence.currently_present,
        is_favorite: presence.is_favorite,
        announcements: ModelRc::new(VecModel::from(announcements)),
        is_self: presence.is_self,
    }
}

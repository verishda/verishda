slint::include_modules!();

mod platform;

fn main() {
    platform::startup();
    let main_window = MainWindow::new().unwrap();
    let main_window_weak = main_window.as_weak();
    main_window.global::<AppUI>().on_login_triggered(move ||{
        // MISSING: actual login via OIDC & Browser
        main_window_weak.unwrap().global::<AppUI>().set_state(MainWindowState::ShowingSitePresenceView);
    });

    main_window.show().unwrap();

    // NOT: will need to change to slint::run_event_loop_until_quit() when we have a systray icon
    slint::run_event_loop().unwrap();
}

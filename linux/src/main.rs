mod ui;

use adw::prelude::*;
use std::{cell::Cell, rc::Rc, time::Duration};

fn main() -> gtk::glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(tungsten_linux::APP_ID)
        .build();
    let failed = Rc::new(Cell::new(false));
    let status = failed.clone();
    let smoke = std::env::args().any(|argument| argument == "--smoke-test");
    application.connect_activate(move |application| {
        if let Err(error) = ui::activate(application) {
            eprintln!("operation=start phase=initialize outcome=failed error={error}");
            status.set(true);
            application.quit();
        } else if smoke {
            let application = application.clone();
            gtk::glib::timeout_add_local_once(Duration::from_millis(800), move || {
                if let Some(window) = application.active_window() {
                    assert!(window.is_mapped(), "the application window did not map");
                    window.close();
                }
                application.quit();
            });
        }
    });
    let result = application.run_with_args(&["tungsten-gtk"]);
    if failed.get() {
        gtk::glib::ExitCode::FAILURE
    } else {
        result
    }
}

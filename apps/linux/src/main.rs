mod card;
mod dialogs;
mod login;
mod model;
mod tray;
mod ui;
mod worker;

use adw::prelude::*;
use std::{cell::RefCell, rc::Rc};

fn main() -> gtk::glib::ExitCode {
    if std::env::args().any(|arg| arg == "--version") {
        println!("hangar-desktop {}", env!("CARGO_PKG_VERSION"));
        return gtk::glib::ExitCode::SUCCESS;
    }
    if gtk::gio::resources_register_include!("hangar.gresource").is_err() {
        eprintln!("无法加载应用资源");
        return gtk::glib::ExitCode::FAILURE;
    }
    // QA stays on the desktop bus for portals, but must never activate a real instance.
    let app_id = if cfg!(debug_assertions) && std::env::var_os("HANGAR_TEST_HOME").is_some() {
        std::env::var("HANGAR_TEST_APP_ID")
            .ok()
            .filter(|id| {
                id.starts_with("dev.mocika.hangar.Qa") && gtk::gio::Application::id_is_valid(id)
            })
            .unwrap_or_else(|| "dev.mocika.hangar".into())
    } else {
        "dev.mocika.hangar".into()
    };
    let app = adw::Application::builder().application_id(&app_id).build();
    app.connect_startup(|_| {
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::PreferLight);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::IconTheme::for_display(&display).add_resource_path("/dev/mocika/hangar/icons");
        }
        gtk::Window::set_default_icon_name("dev.mocika.hangar");
    });
    let state = Rc::new(RefCell::new(None::<Rc<ui::Ui>>));
    let owner = state.clone();
    app.connect_activate(move |app| {
        if let Some(ui) = owner.borrow().as_ref() {
            ui.present();
            return;
        }
        let ui = ui::Ui::new(app);
        ui.present();
        ui.refresh(false);
        *owner.borrow_mut() = Some(ui);
    });
    app.run()
}

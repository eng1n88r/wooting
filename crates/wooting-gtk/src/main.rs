//! Wooting desktop app: lighting, profiles, key map and settings.

mod backend;
mod keyboard_view;
mod window;

use adw::prelude::*;

const APP_ID: &str = "io.github.exbar.Wooting";

fn main() -> gtk::glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(|app| {
        let win = window::build(app);
        win.present();
    });
    app.run()
}

//! Wooting desktop app: Wootility-style sidebar, profile bar, keyboard view.

mod app;
mod backend;
mod keyboard_view;
mod pages;

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
        let win = app::build(app);
        win.present();
    });
    app.run()
}

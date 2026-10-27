//! Gamepad: enable/disable the gamepad USB mode. Button mapping and the
//! analog curve are not captured yet.

use crate::{
    app::{card, page_box, Ctx},
    backend::spawn,
};
use adw::prelude::*;
use serde_json::json;
use std::rc::Rc;

pub fn build(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let title = gtk::Label::new(Some("Gamepad"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    page.append(&title);
    let (en_card, en) = card("Enable Gamepad", Some("Your Wooting keyboard can work just like a controller, sending out analog key signals. Enabling restarts the keyboard with a different USB identity; the app reconnects automatically."));
    let sw = gtk::Switch::new();
    sw.set_halign(gtk::Align::Start);
    en.append(&sw);
    page.append(&en_card);
    let (map_card, map) = card("Controller mapping", Some("Mapping gamepad buttons onto keys and the analog curve editor are not available yet: the protocol for these writes still has to be captured from Wootility."));
    let _ = map;
    page.append(&map_card);

    let loading = Rc::new(std::cell::Cell::new(false));
    let c = ctx.clone();
    let s = sw.clone();
    let ld = loading.clone();
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let s = s.clone();
        let ld = ld.clone();
        spawn(async move {
            if let Some(v) = c.call("settings.device.get", json!({})).await {
                ld.set(true);
                s.set_active(v["gamepad_mode"].as_bool().unwrap_or(false));
                ld.set(false);
            }
        });
    });
    ctx.register_refresher("gamepad", load);
    let c = ctx.clone();
    sw.connect_active_notify(move |s| {
        if loading.get() {
            return;
        }
        let c = c.clone();
        let on = s.is_active();
        spawn(async move {
            if c.call("settings.device.set", json!({"gamepad_mode": on}))
                .await
                .is_some()
            {
                c.toast("Keyboard restarting…");
            }
        });
    });
    page.upcast()
}

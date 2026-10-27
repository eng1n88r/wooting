//! Device-wide settings.

use crate::{
    app::{page_box, Ctx},
    backend::spawn,
};
use adw::prelude::*;
use serde_json::json;
use std::rc::Rc;

pub fn build(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let title = gtk::Label::new(Some("Keyboard Settings"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    page.append(&title);

    let g1 = adw::PreferencesGroup::new();
    let compat = adw::SwitchRow::builder().title("Compatibility Mode (Disable NKRO)").subtitle("Use when your BIOS or a legacy device does not accept the keyboard; restarts the keyboard").build();
    g1.add(&compat);
    let numlock = adw::ComboRow::builder().title("Num Lock indicator behaviour").build();
    numlock.set_model(Some(&gtk::StringList::new(&["Regular: input numbers when the indicator is on", "Inverted: input numbers when the indicator is off"])));
    g1.add(&numlock);
    page.append(&g1);

    let g2 = adw::PreferencesGroup::builder().title("RGB Sleep Mode").description("Minutes of inactivity before the keyboard dims and then turns off the lighting (0 = never).").build();
    let off = adw::SpinRow::with_range(0.0, 240.0, 1.0);
    off.set_title("Turn off after (minutes)");
    let dim = adw::SpinRow::with_range(0.0, 240.0, 1.0);
    dim.set_title("Dim after (minutes)");
    g2.add(&off);
    g2.add(&dim);
    page.append(&g2);

    let g3 = adw::PreferencesGroup::new();
    let debounce = adw::SwitchRow::builder().title("Debounce Filter").subtitle("Ignores unwanted keypresses caused by keys briefly bouncing back when released").build();
    g3.add(&debounce);
    page.append(&g3);

    let g4 = adw::PreferencesGroup::builder().title("Reset all keyboard settings").description("Restores the keyboard to its factory settings. This cannot be reversed.").build();
    let reset = gtk::Button::with_label("Reset all Keyboard Settings");
    reset.add_css_class("destructive-action");
    reset.set_halign(gtk::Align::Start);
    g4.add(&reset);
    page.append(&g4);

    let loading = Rc::new(std::cell::Cell::new(false));
    let c = ctx.clone();
    let w = (compat.clone(), numlock.clone(), off.clone(), dim.clone(), debounce.clone());
    let ld = loading.clone();
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let w = w.clone();
        let ld = ld.clone();
        spawn(async move {
            let g = c.call("settings.global.get", json!({})).await;
            let d = c.call("settings.device.get", json!({})).await;
            ld.set(true);
            if let Some(g) = g {
                w.1.set_selected(if g["numlock_inverted"].as_bool().unwrap_or(false) { 1 } else { 0 });
                w.2.set_value(g["off_after_minutes"].as_f64().unwrap_or(0.0));
                w.3.set_value(g["dim_after_minutes"].as_f64().unwrap_or(0.0));
                w.4.set_active(g["debounce_filter"].as_bool().unwrap_or(false));
            }
            if let Some(d) = d {
                w.0.set_active(d["compat_mode"].as_bool().unwrap_or(false));
            }
            ld.set(false);
        });
    });
    ctx.register_refresher("settings", load.clone());

    let c = ctx.clone();
    let ld = loading.clone();
    compat.connect_active_notify(move |r| {
        if ld.get() { return; }
        let c = c.clone();
        let on = r.is_active();
        spawn(async move {
            if c.call("settings.device.set", json!({"compat_mode": on})).await.is_some() {
                c.toast("Keyboard restarting…");
            }
        });
    });
    let c = ctx.clone();
    let ld = loading.clone();
    numlock.connect_selected_notify(move |r| {
        if ld.get() { return; }
        let c = c.clone();
        let inv = r.selected() == 1;
        spawn(async move { c.call("settings.global.set", json!({"numlock_inverted": inv})).await; });
    });
    let c = ctx.clone();
    let ld = loading.clone();
    debounce.connect_active_notify(move |r| {
        if ld.get() { return; }
        let c = c.clone();
        let on = r.is_active();
        spawn(async move { c.call("settings.global.set", json!({"debounce_filter": on})).await; });
    });
    for (row, key) in [(off.clone(), "off_after_minutes"), (dim.clone(), "dim_after_minutes")] {
        let c = ctx.clone();
        let ld = loading.clone();
        row.connect_value_notify(move |r| {
            if ld.get() { return; }
            let c = c.clone();
            let v = r.value() as u64;
            spawn(async move { c.call("settings.global.set", json!({key: v})).await; });
        });
    }
    let c = ctx.clone();
    reset.connect_clicked(move |b| {
        let c = c.clone();
        let win = b.root().and_downcast::<gtk::Window>();
        spawn(async move {
            let dialog = adw::AlertDialog::new(Some("Reset all keyboard settings?"), Some("Every profile, mapping and setting on the keyboard will be lost. Make a backup first (woot backup)."));
            dialog.add_responses(&[("cancel", "Cancel"), ("reset", "Reset everything")]);
            dialog.set_response_appearance("reset", adw::ResponseAppearance::Destructive);
            if dialog.choose_future(win.as_ref()).await == "reset" {
                if c.call("settings.reset", json!({"confirm": true})).await.is_some() {
                    c.toast("Keyboard reset to factory settings");
                }
                c.refresh_all();
            }
        });
    });
    page.upcast()
}

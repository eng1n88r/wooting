//! Actuation Point page: per-key actuation with live visual feedback.

use crate::{
    app::{card, mm_scale, page_box, Ctx},
    backend::spawn,
};
use adw::prelude::*;
use gtk::glib;
use serde_json::json;
use std::{cell::Cell, rc::Rc};

pub fn build(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let title = gtk::Label::new(Some("Actuation Point"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    page.append(&title);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_homogeneous(true);

    let (set_card, set) = card("Set Actuation Point", Some("Customise the exact distance a key must be pressed before it registers a keypress."));
    let count = gtk::Label::new(Some("Changing actuation point for 0 keys"));
    count.set_xalign(0.0);
    count.add_css_class("accent");
    set.append(&count);
    let scale = mm_scale(0.1, 4.0);
    set.append(&scale);
    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    apply.set_halign(gtk::Align::End);
    let reset = gtk::Button::with_label("Use profile default");
    reset.set_halign(gtk::Align::End);
    let btns = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btns.set_halign(gtk::Align::End);
    btns.append(&reset);
    btns.append(&apply);
    set.append(&btns);
    row.append(&set_card);

    let (fb_card, fb) = card("Visual Feedback", Some("Press a key to test the actuation point. The bar fills with the key's travel."));
    let fb_switch = gtk::Switch::new();
    fb_switch.set_halign(gtk::Align::Start);
    let fb_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    fb_row.append(&gtk::Label::new(Some("Live feedback")));
    fb_row.append(&fb_switch);
    fb.append(&fb_row);
    let pressed = gtk::Label::new(Some("Pressed key: —"));
    pressed.set_xalign(0.0);
    fb.append(&pressed);
    let level = gtk::LevelBar::for_interval(0.0, 255.0);
    level.set_hexpand(true);
    fb.append(&level);
    row.append(&fb_card);
    page.append(&row);

    let c = ctx.clone();
    let cnt = count.clone();
    let sc = scale.clone();
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let cnt = cnt.clone();
        let sc = sc.clone();
        spawn(async move {
            let Some((s, per_key, _)) = super::load_values_overlay(&c).await else { return };
            let sel = c.view.selected();
            cnt.set_text(&format!("Changing actuation point for {} keys", if sel.is_empty() { "all".to_string() } else { sel.len().to_string() }));
            if let Some(first) = sel.first() {
                if let Some(mm) = per_key["keys"][first].as_f64() {
                    sc.set_value(mm);
                    return;
                }
            }
            sc.set_value(s["actuation_mm"].as_f64().unwrap_or(1.6));
        });
    });
    ctx.register_refresher("actuation", load.clone());
    let l = load.clone();
    ctx.view.connect_selection_changed(move || l());

    let c = ctx.clone();
    let s = scale.clone();
    apply.connect_clicked(move |_| {
        let c = c.clone();
        let mm = (s.value() * 100.0).round() / 100.0;
        spawn(async move {
            let sel = c.view.selected();
            if sel.is_empty() {
                c.edit("settings.set", json!({"actuation_mm": mm})).await;
            } else {
                let keys: serde_json::Map<String, serde_json::Value> = sel.into_iter().map(|k| (k, json!(mm))).collect();
                c.edit("actuation.keys.set", json!({"keys": keys})).await;
            }
            c.refresh_page();
        });
    });
    let c = ctx.clone();
    reset.connect_clicked(move |_| {
        let c = c.clone();
        spawn(async move {
            let sel = c.view.selected();
            let keys: serde_json::Map<String, serde_json::Value> = if sel.is_empty() {
                let Some(v) = c.call("actuation.keys.get", json!({})).await else { return };
                v["keys"].as_object().map(|m| m.keys().map(|k| (k.clone(), serde_json::Value::Null)).collect()).unwrap_or_default()
            } else {
                sel.into_iter().map(|k| (k, serde_json::Value::Null)).collect()
            };
            if !keys.is_empty() {
                c.edit("actuation.keys.set", json!({"keys": keys})).await;
            }
            c.refresh_page();
        });
    });

    // Live analog polling while the switch is on.
    let running = Rc::new(Cell::new(false));
    let c = ctx.clone();
    let (pl, lv) = (pressed.clone(), level.clone());
    fb_switch.connect_active_notify(move |sw| {
        running.set(sw.is_active());
        if !sw.is_active() {
            c.view.state.borrow_mut().analog.clear();
            c.view.area.queue_draw();
            return;
        }
        let c = c.clone();
        let running = running.clone();
        let (pl, lv) = (pl.clone(), lv.clone());
        spawn(async move {
            while running.get() {
                if let Ok(v) = c.backend.call("device.analog", json!({})).await {
                    let mut st = c.view.state.borrow_mut();
                    st.analog.clear();
                    let mut best: Option<(String, u8)> = None;
                    for (k, val) in v["pressed"].as_object().into_iter().flatten() {
                        let a = val.as_u64().unwrap_or(0) as u8;
                        st.analog.insert(k.clone(), a);
                        if best.as_ref().map(|b| a > b.1).unwrap_or(true) {
                            best = Some((k.clone(), a));
                        }
                    }
                    drop(st);
                    c.view.area.queue_draw();
                    match best {
                        Some((k, a)) => {
                            pl.set_text(&format!("Pressed key: {k}"));
                            lv.set_value(a as f64);
                        }
                        None => {
                            pl.set_text("Pressed key: —");
                            lv.set_value(0.0);
                        }
                    }
                }
                glib::timeout_future(std::time::Duration::from_millis(80)).await;
            }
        });
    });
    page.upcast()
}

//! Quick Settings: actuation, rapid trigger and Tachyon for selected keys.

use crate::{
    app::{card, mm_scale, on_scale_settled, page_box, set_scale_quiet, Ctx},
    backend::spawn,
};
use adw::prelude::*;
use gtk::glib;
use serde_json::json;
use std::rc::Rc;

pub fn build(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let title = gtk::Label::new(Some("Quick Settings"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    page.append(&title);
    let hint = gtk::Label::new(Some("Select keys on the keyboard to change them; with no selection the whole keyboard is changed."));
    hint.add_css_class("dim-label");
    hint.set_xalign(0.0);
    page.append(&hint);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_homogeneous(true);

    // ----- actuation card
    let (act_card, act) = card(
        "Actuation Point",
        Some("Set the point to activate a keypress for the selected keys."),
    );
    let act_scale = mm_scale(0.1, 4.0);
    act.append(&act_scale);
    let act_apply = gtk::Button::with_label("Apply to selection");
    act_apply.set_halign(gtk::Align::End);
    act.append(&act_apply);
    row.append(&act_card);

    // ----- rapid trigger card
    let (rt_card, rt) = card(
        "Rapid Trigger",
        Some("A key rapidly activates and resets based on intended press or release."),
    );
    let rt_switch = gtk::Switch::new();
    rt_switch.set_halign(gtk::Align::End);
    let rt_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    rt_row.append(&gtk::Label::new(Some("Enable for selection")));
    let sp = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    sp.set_hexpand(true);
    rt_row.append(&sp);
    rt_row.append(&rt_switch);
    rt.append(&rt_row);
    let split_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    split_row.append(&gtk::Label::new(Some("Split sensitivity")));
    let sp2 = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    sp2.set_hexpand(true);
    split_row.append(&sp2);
    let split = gtk::Switch::new();
    split_row.append(&split);
    rt.append(&split_row);
    let press_label = gtk::Label::new(Some("Sensitivity (press)"));
    press_label.set_xalign(0.0);
    press_label.add_css_class("dim-label");
    rt.append(&press_label);
    let press = mm_scale(0.1, 4.0);
    rt.append(&press);
    let rel_label = gtk::Label::new(Some("Sensitivity (release)"));
    rel_label.set_xalign(0.0);
    rel_label.add_css_class("dim-label");
    rt.append(&rel_label);
    let release = mm_scale(0.1, 4.0);
    rt.append(&release);
    row.append(&rt_card);

    // ----- tachyon card
    let (ta_card, ta) = card(
        "Tachyon Mode",
        Some("Optimises the keyboard for input speed. Limits brightness and disables RGB effects."),
    );
    let ta_switch = gtk::Switch::new();
    ta_switch.set_halign(gtk::Align::End);
    let ta_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    ta_row.append(&gtk::Label::new(Some("Enabled")));
    let sp3 = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    sp3.set_hexpand(true);
    ta_row.append(&sp3);
    ta_row.append(&ta_switch);
    ta.append(&ta_row);
    row.append(&ta_card);
    page.append(&row);

    // ----- settled handlers (registered before the loader so it can silence them)
    let c = ctx.clone();
    let press_id = Rc::new(on_scale_settled(&press, move |v| {
        let c = c.clone();
        spawn(async move {
            c.edit(
                "settings.set",
                json!({"rt_press_mm": (v * 100.0).round() / 100.0}),
            )
            .await;
        });
    }));
    let c = ctx.clone();
    let release_id = Rc::new(on_scale_settled(&release, move |v| {
        let c = c.clone();
        spawn(async move {
            c.edit(
                "settings.set",
                json!({"rt_release_mm": (v * 100.0).round() / 100.0}),
            )
            .await;
        });
    }));

    // ----- loading
    let loading = Rc::new(std::cell::Cell::new(false));
    let c = ctx.clone();
    let w = (
        act_scale.clone(),
        rt_switch.clone(),
        split.clone(),
        press.clone(),
        release.clone(),
        ta_switch.clone(),
        rel_label.clone(),
    );
    let ld = loading.clone();
    let (pid, rid) = (press_id.clone(), release_id.clone());
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let w = w.clone();
        let ld = ld.clone();
        let (pid, rid) = (pid.clone(), rid.clone());
        spawn(async move {
            let Some((s, _, rt)) = super::load_values_overlay(&c).await else {
                return;
            };
            ld.set(true);
            w.0.set_value(s["actuation_mm"].as_f64().unwrap_or(1.6));
            let sel = c.view.selected();
            let rt_keys: Vec<String> = rt["keys"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            let rt_all = s["rapid_trigger"].as_bool().unwrap_or(false);
            w.1.set_active(if sel.is_empty() {
                rt_all
            } else {
                rt_all || sel.iter().all(|k| rt_keys.contains(k))
            });
            let rel = s["rt_release_mm"].as_f64();
            w.2.set_active(rel.is_some());
            set_scale_quiet(&w.3, &pid, s["rt_press_mm"].as_f64().unwrap_or(0.4));
            set_scale_quiet(
                &w.4,
                &rid,
                rel.unwrap_or(s["rt_press_mm"].as_f64().unwrap_or(0.4)),
            );
            w.4.set_visible(rel.is_some());
            w.6.set_visible(rel.is_some());
            w.5.set_active(s["tachyon"].as_bool().unwrap_or(false));
            ld.set(false);
        });
    });
    ctx.register_refresher("quick", load.clone());
    let l = load.clone();
    ctx.view.connect_selection_changed(move || l());

    // ----- actions
    let c = ctx.clone();
    let s = act_scale.clone();
    act_apply.connect_clicked(move |_| {
        let c = c.clone();
        let mm = (s.value() * 100.0).round() / 100.0;
        spawn(async move {
            let sel = c.view.selected();
            if sel.is_empty() {
                c.edit("settings.set", json!({"actuation_mm": mm})).await;
            } else {
                let keys: serde_json::Map<String, serde_json::Value> =
                    sel.into_iter().map(|k| (k, json!(mm))).collect();
                c.edit("actuation.keys.set", json!({"keys": keys})).await;
            }
            c.refresh_page();
        });
    });
    let c = ctx.clone();
    let ld = loading.clone();
    rt_switch.connect_active_notify(move |sw| {
        if ld.get() {
            return;
        }
        let c = c.clone();
        let on = sw.is_active();
        spawn(async move {
            let sel = c.view.selected();
            if sel.is_empty() {
                c.edit("settings.set", json!({"rapid_trigger": on})).await;
            } else {
                let keys: serde_json::Map<String, serde_json::Value> =
                    sel.into_iter().map(|k| (k, json!(on))).collect();
                c.edit("rapid_trigger.keys.set", json!({"keys": keys}))
                    .await;
            }
            c.refresh_page();
        });
    });
    let c = ctx.clone();
    let ld = loading.clone();
    let (pr, rl) = (press.clone(), release.clone());
    split.connect_active_notify(move |sw| {
        if ld.get() {
            return;
        }
        let c = c.clone();
        let v = if sw.is_active() {
            json!(rl.value())
        } else {
            serde_json::Value::Null
        };
        let _ = &pr;
        spawn(async move {
            c.edit("settings.set", json!({"rt_release_mm": v})).await;
            c.refresh_page();
        });
    });
    let c = ctx.clone();
    let ld = loading.clone();
    ta_switch.connect_active_notify(move |sw| {
        if ld.get() {
            return;
        }
        let c = c.clone();
        let on = sw.is_active();
        spawn(async move {
            c.edit("settings.set", json!({"tachyon": on})).await;
            c.refresh_page();
        });
    });
    let _ = glib::MainContext::default();
    page.upcast()
}

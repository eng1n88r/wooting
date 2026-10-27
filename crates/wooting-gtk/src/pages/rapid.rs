//! Rapid Trigger page.

use crate::{
    app::{card, mm_scale, on_scale_settled, page_box, set_scale_quiet, Ctx},
    backend::spawn,
};
use adw::prelude::*;
use serde_json::json;
use std::{cell::Cell, rc::Rc};

pub fn build(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let title = gtk::Label::new(Some("Rapid Trigger"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    page.append(&title);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_homogeneous(true);

    let (en_card, en) = card("Enable Rapid Trigger", Some("Rapid Trigger dynamically actuates and resets your key based on your intention to press or release the key."));
    let en_switch = gtk::Switch::new();
    en_switch.set_halign(gtk::Align::Start);
    let en_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    en_row.append(&gtk::Label::new(Some(
        "Enable for selection (all keys when nothing is selected)",
    )));
    en_row.append(&en_switch);
    en.append(&en_row);
    let count = gtk::Label::new(Some("Enabled on 0 keys"));
    count.set_xalign(0.0);
    count.add_css_class("dim-label");
    en.append(&count);
    row.append(&en_card);

    let (se_card, se) = card("Rapid Trigger Sensitivity", None);
    let split_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    split_row.append(&gtk::Label::new(Some("Split sensitivity")));
    let split = gtk::Switch::new();
    split_row.append(&split);
    se.append(&split_row);
    let pl = gtk::Label::new(Some("Key press – activate"));
    pl.set_xalign(0.0);
    pl.add_css_class("dim-label");
    se.append(&pl);
    let press = mm_scale(0.1, 4.0);
    se.append(&press);
    let rl = gtk::Label::new(Some("Key release – deactivate"));
    rl.set_xalign(0.0);
    rl.add_css_class("dim-label");
    se.append(&rl);
    let release = mm_scale(0.1, 4.0);
    se.append(&release);
    row.append(&se_card);

    let (co_card, co) = card("Continuous Rapid Trigger", Some("When enabled, Rapid Trigger ends when the entire key is released. When disabled, it ends at the actuation point."));
    let cont = gtk::Switch::new();
    cont.set_halign(gtk::Align::Start);
    co.append(&cont);
    row.append(&co_card);
    page.append(&row);

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
    let loading = Rc::new(Cell::new(false));
    let c = ctx.clone();
    let w = (
        en_switch.clone(),
        count.clone(),
        split.clone(),
        press.clone(),
        release.clone(),
        rl.clone(),
        cont.clone(),
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
            let rt_keys: Vec<String> = rt["keys"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            let rt_all = s["rapid_trigger"].as_bool().unwrap_or(false);
            let sel = c.view.selected();
            w.0.set_active(if sel.is_empty() {
                rt_all
            } else {
                rt_all || sel.iter().all(|k| rt_keys.contains(k))
            });
            w.1.set_text(&if rt_all {
                "Enabled on all keys".to_string()
            } else {
                format!("Enabled on {} keys", rt_keys.len())
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
            w.5.set_visible(rel.is_some());
            w.6.set_active(s["continuous_rt"].as_bool().unwrap_or(false));
            ld.set(false);
        });
    });
    ctx.register_refresher("rapid", load.clone());
    let l = load.clone();
    ctx.view.connect_selection_changed(move || l());

    let c = ctx.clone();
    let ld = loading.clone();
    en_switch.connect_active_notify(move |sw| {
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
    let rel = release.clone();
    split.connect_active_notify(move |sw| {
        if ld.get() {
            return;
        }
        let c = c.clone();
        let v = if sw.is_active() {
            json!(rel.value())
        } else {
            serde_json::Value::Null
        };
        spawn(async move {
            c.edit("settings.set", json!({"rt_release_mm": v})).await;
            c.refresh_page();
        });
    });
    let c = ctx.clone();
    let ld = loading.clone();
    cont.connect_active_notify(move |sw| {
        if ld.get() {
            return;
        }
        let c = c.clone();
        let on = sw.is_active();
        spawn(async move {
            c.edit("settings.set", json!({"continuous_rt": on})).await;
            c.refresh_page();
        });
    });
    page.upcast()
}

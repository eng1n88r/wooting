//! Advanced Keys: Rappy Snappy, SOCD, DKS, Mod Tap, Toggle.

use crate::{
    app::{card, code_choices, code_dropdown, label_for, page_box, Ctx},
    backend::spawn,
};
use adw::prelude::*;
use serde_json::json;
use std::rc::Rc;

const KINDS: &[(&str, &str, &str, &str)] = &[
    (
        "rappy_snappy",
        "Rappy Snappy",
        "RS",
        "Monitors the 2 selected keys and activates whichever key is pressed down further.",
    ),
    (
        "socd",
        "Snappy Tappy (SOCD)",
        "ST",
        "Monitors the 2 selected keys and activates them based on your chosen settings.",
    ),
    (
        "dks",
        "Dynamic Keystroke (DKS)",
        "DKS",
        "4 different actions on a single key based on key position.",
    ),
    (
        "mod_tap",
        "Mod Tap",
        "MT",
        "2 actions on a single key: hold for the first action, tap for the second.",
    ),
    (
        "toggle",
        "Toggle Key",
        "TGL",
        "Lock an action between on and off by tapping; holding keeps the normal key behaviour.",
    ),
];

pub fn build(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let title = gtk::Label::new(Some("Advanced Keys"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    page.append(&title);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_homogeneous(true);

    let (add_card, add) = card(
        "Add Advanced Key",
        Some("Select the key(s) on the keyboard, pick a type, fill in the bindings and assign."),
    );
    let kind_list = gtk::ListBox::new();
    kind_list.add_css_class("boxed-list");
    for (id, name, _, desc) in KINDS {
        let r = adw::ActionRow::builder()
            .title(*name)
            .subtitle(*desc)
            .activatable(true)
            .build();
        r.set_widget_name(id);
        kind_list.append(&r);
    }
    add.append(&kind_list);
    let form = gtk::Box::new(gtk::Orientation::Vertical, 8);
    add.append(&form);
    row.append(&add_card);

    let (list_card, list) = card("Active Advanced Keys", None);
    let count = gtk::Label::new(Some("0 / 40"));
    count.set_xalign(0.0);
    count.add_css_class("dim-label");
    list.append(&count);
    let entries = gtk::ListBox::new();
    entries.add_css_class("boxed-list");
    entries.set_selection_mode(gtk::SelectionMode::None);
    list.append(&entries);
    row.append(&list_card);
    page.append(&row);

    let c = ctx.clone();
    let (en, cnt) = (entries.clone(), count.clone());
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let (en, cnt) = (en.clone(), cnt.clone());
        spawn(async move {
            while let Some(ch) = en.first_child() {
                en.remove(&ch);
            }
            let Some(v) = c.call("advanced_keys.get", json!({})).await else {
                return;
            };
            let items = v["entries"].as_array().cloned().unwrap_or_default();
            cnt.set_text(&format!("{} / 40", items.len()));
            let mut badges = std::collections::HashMap::new();
            for e in &items {
                let key = e["key"].as_str().unwrap_or("?").to_string();
                let kind = e["kind"].as_str().unwrap_or("?");
                let badge = KINDS
                    .iter()
                    .find(|k| k.0 == kind)
                    .map(|k| k.2)
                    .unwrap_or("?");
                badges.insert(key.clone(), badge.to_string());
                let subtitle = match kind {
                    "mod_tap" => format!(
                        "Mod Tap · hold {} · tap {}",
                        label_for(e["hold"].as_str().unwrap_or("")),
                        label_for(e["tap"].as_str().unwrap_or(""))
                    ),
                    "toggle" => format!("Toggle · {}", label_for(e["tap"].as_str().unwrap_or(""))),
                    "rappy_snappy" => format!(
                        "Rappy Snappy with {}",
                        e["other_key"].as_str().unwrap_or("?")
                    ),
                    "socd" => format!(
                        "SOCD with {} (mode {})",
                        e["other_key"].as_str().unwrap_or("?"),
                        e["mode"]
                    ),
                    "dks" => format!(
                        "DKS · {}",
                        e["dks_codes"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|c| c.as_str())
                            .map(label_for)
                            .collect::<Vec<_>>()
                            .join(" / ")
                    ),
                    _ => "Unknown kind".to_string(),
                };
                let r = adw::ActionRow::builder()
                    .title(&key)
                    .subtitle(subtitle)
                    .build();
                let del = gtk::Button::from_icon_name("user-trash-symbolic");
                del.add_css_class("flat");
                del.set_valign(gtk::Align::Center);
                let c2 = c.clone();
                del.connect_clicked(move |_| {
                    let c2 = c2.clone();
                    let key = key.clone();
                    spawn(async move {
                        c2.edit("advanced_keys.remove", json!({"key": key})).await;
                        c2.refresh_page();
                    });
                });
                r.add_suffix(&del);
                en.append(&r);
            }
            c.view.state.borrow_mut().badges = badges;
            c.view.area.queue_draw();
        });
    });
    ctx.register_refresher("advanced", load.clone());

    // Kind form.
    let choices = Rc::new(code_choices());
    let c = ctx.clone();
    let f = form.clone();
    kind_list.connect_row_activated(move |_, r| {
        let kind = r.widget_name().to_string();
        while let Some(ch) = f.first_child() {
            f.remove(&ch);
        }
        let title = gtk::Label::new(Some(
            KINDS
                .iter()
                .find(|k| k.0 == kind)
                .map(|k| k.1)
                .unwrap_or(""),
        ));
        title.add_css_class("card-title");
        title.set_xalign(0.0);
        f.append(&title);
        let mut dropdowns: Vec<(String, gtk::DropDown)> = Vec::new();
        let add_dd = |f: &gtk::Box,
                      dropdowns: &mut Vec<(String, gtk::DropDown)>,
                      label: &str,
                      key: &str,
                      default: &str| {
            let hb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let l = gtk::Label::new(Some(label));
            l.set_xalign(0.0);
            l.set_hexpand(true);
            let dd = code_dropdown(&choices);
            if let Some(i) = choices.iter().position(|(n, _)| n == default) {
                dd.set_selected(i as u32);
            }
            hb.append(&l);
            hb.append(&dd);
            f.append(&hb);
            dropdowns.push((key.to_string(), dd));
        };
        let hint = gtk::Label::new(Some(match kind.as_str() {
            "rappy_snappy" | "socd" => {
                "Select exactly 2 keys on the keyboard (first = the key, second = its partner)."
            }
            _ => "Select exactly 1 key on the keyboard.",
        }));
        hint.add_css_class("dim-label");
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        f.append(&hint);
        let mode = gtk::SpinButton::with_range(0.0, 3.0, 1.0);
        match kind.as_str() {
            "mod_tap" => {
                add_dd(&f, &mut dropdowns, "Hold", "hold", "lctrl");
                add_dd(&f, &mut dropdowns, "Tap", "tap", "esc");
            }
            "toggle" => add_dd(&f, &mut dropdowns, "Toggle", "code", "fn1"),
            "dks" => {
                add_dd(&f, &mut dropdowns, "1 · start of press", "c0", "a");
                add_dd(&f, &mut dropdowns, "2 · fully pressed", "c1", "none");
                add_dd(&f, &mut dropdowns, "3 · starts releasing", "c2", "none");
                add_dd(&f, &mut dropdowns, "4 · fully released", "c3", "none");
            }
            "socd" => {
                mode.set_value(1.0);
                let hb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let l = gtk::Label::new(Some("Mode (1 = last input wins)"));
                l.set_xalign(0.0);
                l.set_hexpand(true);
                hb.append(&l);
                hb.append(&mode);
                f.append(&hb);
            }
            _ => {}
        }
        let assign = gtk::Button::with_label("Assign to selected key");
        assign.add_css_class("suggested-action");
        assign.set_halign(gtk::Align::End);
        f.append(&assign);
        let c = c.clone();
        let ch = choices.clone();
        let md = mode.clone();
        assign.connect_clicked(move |_| {
            let sel = c.view.selected();
            let two = matches!(kind.as_str(), "rappy_snappy" | "socd");
            if (two && sel.len() != 2) || (!two && sel.len() != 1) {
                c.toast(if two {
                    "Select exactly two keys"
                } else {
                    "Select exactly one key"
                });
                return;
            }
            let name_of = |dd: &gtk::DropDown| ch[dd.selected() as usize].0.clone();
            let mut params = json!({"key": sel[0], "kind": kind});
            for (k, dd) in &dropdowns {
                params[k] = name_of(dd).into();
            }
            if kind == "dks" {
                params["codes"] = json!([params["c0"], params["c1"], params["c2"], params["c3"]]);
            }
            if two {
                params["other_key"] = sel[1].clone().into();
            }
            if kind == "socd" {
                params["mode"] = (md.value() as u64).into();
            }
            let c = c.clone();
            spawn(async move {
                if c.edit("advanced_keys.set", params).await.is_some() {
                    c.toast("Advanced key added");
                }
                c.refresh_page();
            });
        });
    });
    page.upcast()
}

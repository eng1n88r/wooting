//! Remap: layers, presets, un-linked toggle and the key palette.

use crate::{
    app::{card, key_palette, page_box, Ctx},
    backend::spawn,
};
use adw::prelude::*;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};

pub fn build(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let title = gtk::Label::new(Some("Remap"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    page.append(&title);

    // Layer strip.
    let strip = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let layer_names = ["Main layer", "Fn layer 1", "Fn layer 2", "Fn layer 3"];
    let current: Rc<RefCell<u8>> = Rc::new(RefCell::new(0));
    let mut layer_buttons = Vec::new();
    for (i, n) in layer_names.iter().enumerate() {
        let b = gtk::ToggleButton::with_label(n);
        b.set_active(i == 0);
        strip.append(&b);
        layer_buttons.push(b);
    }
    for i in 1..layer_buttons.len() {
        layer_buttons[i].set_group(Some(&layer_buttons[0]));
    }
    let clipboard: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
    let copy = gtk::Button::with_label("Copy");
    let paste = gtk::Button::with_label("Paste");
    paste.set_sensitive(false);
    strip.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    strip.append(&copy);
    strip.append(&paste);
    page.append(&strip);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let (left_card, left) = card("Remap keys", None);
    left_card.set_size_request(300, -1);
    let linked_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let linked_label = gtk::Label::new(Some("Custom layout (un-linked)"));
    let linked = gtk::Switch::new();
    linked_row.append(&linked_label);
    let sp = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    sp.set_hexpand(true);
    linked_row.append(&sp);
    linked_row.append(&linked);
    left.append(&linked_row);
    let linked_hint = gtk::Label::new(Some("Off: this layer follows the keyboard's factory layout. On: the layer is stored in the profile and can be changed."));
    linked_hint.add_css_class("dim-label");
    linked_hint.set_wrap(true);
    linked_hint.set_xalign(0.0);
    left.append(&linked_hint);
    let howto = gtk::Label::new(Some("Select keys on the keyboard, then click a key on the right. Or pick a preset below."));
    howto.set_wrap(true);
    howto.set_xalign(0.0);
    left.append(&howto);
    let pl = gtk::Label::new(Some("Select a preset"));
    pl.set_xalign(0.0);
    pl.add_css_class("dim-label");
    left.append(&pl);
    let presets = gtk::FlowBox::new();
    presets.set_selection_mode(gtk::SelectionMode::None);
    presets.set_max_children_per_line(4);
    for (id, label) in [("default", "Default"), ("function", "Function"), ("dvorak", "Dvorak"), ("colemak", "Colemak"), ("workman", "Workman"), ("abc", "ABC")] {
        let b = gtk::Button::with_label(label);
        b.add_css_class("keytile");
        let c = ctx.clone();
        let cur = current.clone();
        b.connect_clicked(move |_| {
            let c = c.clone();
            let layer = *cur.borrow();
            spawn(async move {
                c.edit("keymap.set_layer", json!({"layer": layer, "preset": id})).await;
                c.refresh_page();
            });
        });
        presets.insert(&b, -1);
    }
    left.append(&presets);
    row.append(&left_card);

    let (right_card, right) = card("Keys", None);
    right_card.set_hexpand(true);
    let c = ctx.clone();
    let cur = current.clone();
    let palette = key_palette(Rc::new(move |name: String| {
        let keys = c.view.selected();
        if keys.is_empty() {
            c.toast("Select some keys on the keyboard first");
            return;
        }
        let map: serde_json::Map<String, Value> = keys.into_iter().map(|k| (k, Value::String(name.clone()))).collect();
        let c = c.clone();
        let layer = *cur.borrow();
        spawn(async move {
            c.edit("keymap.set", json!({"keys": map, "layer": layer})).await;
            c.refresh_page();
        });
    }));
    right.append(&palette);
    row.append(&right_card);
    page.append(&row);

    // Loading.
    let loading = Rc::new(std::cell::Cell::new(false));
    let c = ctx.clone();
    let cur = current.clone();
    let lk = linked.clone();
    let ld = loading.clone();
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let layer = *cur.borrow();
        let lk = lk.clone();
        let ld = ld.clone();
        spawn(async move {
            let Some(v) = c.call("keymap.get", json!({"layer": layer})).await else { return };
            ld.set(true);
            lk.set_active(v["stored"].as_bool().unwrap_or(false));
            ld.set(false);
            let mut st = c.view.state.borrow_mut();
            st.codes.clear();
            if let Some(keys) = v["keys"].as_object() {
                for (id, k) in keys {
                    let name = k["name"].as_str().unwrap_or("?").to_string();
                    let label = k["label"].as_str().unwrap_or(&name).to_string();
                    st.codes.insert(id.clone(), (name, label));
                }
            }
            drop(st);
            c.view.area.queue_draw();
        });
    });
    ctx.register_refresher("remap", load.clone());
    for (i, b) in layer_buttons.iter().enumerate() {
        let cur = current.clone();
        let l = load.clone();
        b.connect_toggled(move |btn| {
            if btn.is_active() {
                *cur.borrow_mut() = i as u8;
                l();
            }
        });
    }
    let c = ctx.clone();
    let cur = current.clone();
    let ld = loading.clone();
    linked.connect_active_notify(move |sw| {
        if ld.get() {
            return;
        }
        let c = c.clone();
        let layer = *cur.borrow();
        let on = sw.is_active();
        spawn(async move {
            if on {
                // Store the current (default) rows so the layer becomes editable.
                if let Some(v) = c.call("keymap.get", json!({"layer": layer})).await {
                    c.edit("keymap.set_layer", json!({"layer": layer, "rows": v["rows"]})).await;
                }
            } else {
                c.edit("keymap.reset", json!({"layer": layer})).await;
            }
            c.refresh_page();
        });
    });
    let c = ctx.clone();
    let cur = current.clone();
    let cb = clipboard.clone();
    let pb = paste.clone();
    copy.connect_clicked(move |_| {
        let c = c.clone();
        let cb = cb.clone();
        let pb = pb.clone();
        let layer = *cur.borrow();
        spawn(async move {
            if let Some(v) = c.call("keymap.get", json!({"layer": layer})).await {
                *cb.borrow_mut() = Some(v["rows"].clone());
                pb.set_sensitive(true);
                c.toast(format!("Copied {}", layer_names[layer as usize]));
            }
        });
    });
    let c = ctx.clone();
    let cur = current.clone();
    let cb = clipboard.clone();
    paste.connect_clicked(move |_| {
        let Some(rows) = cb.borrow().clone() else { return };
        let c = c.clone();
        let layer = *cur.borrow();
        spawn(async move {
            c.edit("keymap.set_layer", json!({"layer": layer, "rows": rows})).await;
            c.refresh_page();
        });
    });
    page.upcast()
}

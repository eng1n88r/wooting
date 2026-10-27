//! My Profiles: onboard profile cards with actions, new profile, import.

use crate::{
    app::{card, page_box, Ctx},
    backend::spawn,
    keyboard_view::{gdk_to_rgb, rgb_to_gdk},
};
use adw::prelude::*;
use gtk::{gdk, gio};
use serde_json::{json, Value};
use std::rc::Rc;
use wooting_proto::Rgb;

pub fn build(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some("My Profiles"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    head.append(&title);
    let import = gtk::Button::with_label("Import profile…");
    let new = gtk::Button::with_label("New profile");
    new.add_css_class("suggested-action");
    head.append(&import);
    head.append(&new);
    page.append(&head);

    let (ob_card, ob) = card("Onboard profiles", Some("Profiles stored on the keyboard (4 slots). Slot 1 is the default profile."));
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_max_children_per_line(2);
    grid.set_min_children_per_line(2);
    grid.set_homogeneous(true);
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    ob.append(&grid);
    page.append(&ob_card);

    let c = ctx.clone();
    let g = grid.clone();
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let g = g.clone();
        spawn(async move {
            while let Some(ch) = g.first_child() {
                g.remove(&ch);
            }
            let Some(list) = c.call("profile.list", json!({})).await else { return };
            let n = list.as_array().map(|a| a.len()).unwrap_or(0);
            for p in list.as_array().into_iter().flatten() {
                g.insert(&profile_card(&c, p), -1);
            }
            if n < 4 {
                let empty = gtk::Label::new(Some("Free slot"));
                empty.add_css_class("dim-label");
                empty.set_margin_top(20);
                empty.set_margin_bottom(20);
                g.insert(&empty, -1);
            }
        });
    });
    ctx.register_refresher("profiles", load.clone());

    let c = ctx.clone();
    new.connect_clicked(move |b| {
        let c = c.clone();
        let win = b.root().and_downcast::<gtk::Window>();
        spawn(async move {
            let Some(list) = c.call("profile.list", json!({})).await else { return };
            let names: Vec<String> = list.as_array().into_iter().flatten().map(|p| format!("Copy of {}", p["name"].as_str().unwrap_or("?"))).collect();
            let dialog = adw::AlertDialog::new(Some("New profile"), Some("Stored on the keyboard in the next free slot."));
            let bx = gtk::Box::new(gtk::Orientation::Vertical, 8);
            let entry = gtk::Entry::new();
            entry.set_placeholder_text(Some("Profile name"));
            let from = gtk::DropDown::from_strings(&names.iter().map(|s| s.as_str()).collect::<Vec<_>>());
            let colour = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
            colour.set_rgba(&rgb_to_gdk(Rgb::new(255, 212, 92)));
            let cr = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            cr.append(&gtk::Label::new(Some("Badge colour")));
            cr.append(&colour);
            bx.append(&entry);
            bx.append(&from);
            bx.append(&cr);
            dialog.set_extra_child(Some(&bx));
            dialog.add_responses(&[("cancel", "Cancel"), ("create", "Create profile")]);
            dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
            if dialog.choose_future(win.as_ref()).await == "create" {
                let name = entry.text().to_string();
                if name.trim().is_empty() {
                    c.toast("Give the profile a name");
                    return;
                }
                if c.call("profile.create", json!({"name": name, "from_slot": from.selected(), "color": gdk_to_rgb(&colour.rgba()).to_hex()})).await.is_some() {
                    c.toast("Profile created");
                }
                c.refresh_bar().await;
                c.refresh_page();
            }
        });
    });
    let c = ctx.clone();
    import.connect_clicked(move |b| {
        let c = c.clone();
        let win = b.root().and_downcast::<gtk::Window>();
        let dialog = gtk::FileDialog::builder().title("Import profile JSON").build();
        dialog.open(win.as_ref(), gio::Cancellable::NONE, move |res| {
            let Ok(file) = res else { return };
            let Some(path) = file.path() else { return };
            spawn(async move {
                let Ok(text) = std::fs::read_to_string(&path) else { c.toast("Could not read file"); return };
                let Ok(v) = serde_json::from_str::<Value>(&text) else { c.toast("Not a JSON profile"); return };
                let data = v.get("data").cloned().unwrap_or(v);
                let Some(list) = c.call("profile.list", json!({})).await else { return };
                let slot = list.as_array().map(|a| a.len()).unwrap_or(0);
                if slot >= 4 {
                    c.toast("All 4 slots are in use; delete one first");
                    return;
                }
                if c.call("profile.import", json!({"slot": slot, "data": data})).await.is_some() {
                    c.toast("Profile imported");
                }
                c.refresh_bar().await;
                c.refresh_page();
            });
        });
    });
    page.upcast()
}

fn profile_card(ctx: &Ctx, p: &Value) -> gtk::Widget {
    let slot = p["slot"].as_u64().unwrap_or(0) as u8;
    let name = p["name"].as_str().unwrap_or("?").to_string();
    let active = p["active"].as_bool().unwrap_or(false);
    let dirty = p["dirty"].as_bool().unwrap_or(false);
    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    bx.add_css_class("card");
    bx.set_margin_top(2);
    let swatch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    swatch.set_valign(gtk::Align::Center);
    swatch.set_margin_start(10);
    let css = gtk::CssProvider::new();
    css.load_from_string(&format!(
        ".pcard-{slot} {{ background: rgb({},{},{}); border-radius: 50%; min-width: 26px; min-height: 26px; }}",
        p["color"]["r"].as_u64().unwrap_or(120), p["color"]["g"].as_u64().unwrap_or(120), p["color"]["b"].as_u64().unwrap_or(120)
    ));
    gtk::style_context_add_provider_for_display(&gdk::Display::default().unwrap(), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    swatch.add_css_class(&format!("pcard-{slot}"));
    bx.append(&swatch);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_margin_top(10);
    text.set_margin_bottom(10);
    text.set_hexpand(true);
    let t = gtk::Label::new(Some(&format!("{}  {}{}", slot + 1, name, if dirty { " •" } else { "" })));
    t.add_css_class("card-title");
    t.set_xalign(0.0);
    let s = gtk::Label::new(Some(&format!("{}{}", if slot == 0 { "Default · " } else { "" }, if active { "Active" } else { "Click ⋮ for actions" })));
    s.add_css_class("dim-label");
    s.set_xalign(0.0);
    text.append(&t);
    text.append(&s);
    bx.append(&text);
    let menu = gio::Menu::new();
    menu.append(Some("Activate"), Some(&format!("profile.activate({slot})")));
    menu.append(Some("Rename…"), Some(&format!("profile.rename({slot})")));
    menu.append(Some("Duplicate"), Some(&format!("profile.duplicate({slot})")));
    menu.append(Some("Set as default"), Some(&format!("profile.default({slot})")));
    menu.append(Some("Export…"), Some(&format!("profile.export({slot})")));
    menu.append(Some("Delete…"), Some(&format!("profile.delete({slot})")));
    let mb = gtk::MenuButton::builder().icon_name("view-more-symbolic").menu_model(&menu).valign(gtk::Align::Center).build();
    mb.add_css_class("flat");
    mb.set_margin_end(6);
    bx.append(&mb);
    install_actions(ctx, &bx, slot, name);
    bx.upcast()
}

fn install_actions(ctx: &Ctx, widget: &gtk::Box, slot: u8, name: String) {
    let group = gio::SimpleActionGroup::new();
    let make = |id: &str, f: Box<dyn Fn()>| {
        let a = gio::SimpleAction::new(id, Some(glib_variant_type()));
        a.connect_activate(move |_, _| f());
        a
    };
    let c = ctx.clone();
    group.add_action(&make("activate", Box::new(move || {
        let c = c.clone();
        spawn(async move { c.switch_profile(slot).await; });
    })));
    let c = ctx.clone();
    let w = widget.clone();
    let n = name.clone();
    group.add_action(&make("rename", Box::new(move || {
        let c = c.clone();
        let win = w.root().and_downcast::<gtk::Window>();
        let n = n.clone();
        spawn(async move {
            let dialog = adw::AlertDialog::new(Some("Rename profile"), None);
            let entry = gtk::Entry::new();
            entry.set_text(&n);
            dialog.set_extra_child(Some(&entry));
            dialog.add_responses(&[("cancel", "Cancel"), ("ok", "Rename")]);
            dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
            if dialog.choose_future(win.as_ref()).await == "ok" {
                c.call("profile.rename", json!({"slot": slot, "name": entry.text().to_string()})).await;
                c.refresh_bar().await;
                c.refresh_page();
            }
        });
    })));
    let c = ctx.clone();
    let n = name.clone();
    group.add_action(&make("duplicate", Box::new(move || {
        let c = c.clone();
        let n = n.clone();
        spawn(async move {
            if c.call("profile.create", json!({"name": format!("{n} copy"), "from_slot": slot})).await.is_some() {
                c.toast("Profile duplicated");
            }
            c.refresh_bar().await;
            c.refresh_page();
        });
    })));
    let c = ctx.clone();
    group.add_action(&make("default", Box::new(move || {
        let c = c.clone();
        spawn(async move {
            if slot == 0 {
                c.toast("Already the default profile");
                return;
            }
            if c.call("profile.swap", json!({"a": 0, "b": slot})).await.is_some() {
                c.toast("Default profile changed (slots swapped)");
            }
            c.refresh_bar().await;
            c.refresh_all();
        });
    })));
    let c = ctx.clone();
    let w = widget.clone();
    let n = name.clone();
    group.add_action(&make("export", Box::new(move || {
        let c = c.clone();
        let win = w.root().and_downcast::<gtk::Window>();
        let n = n.clone();
        let dialog = gtk::FileDialog::builder().title("Export profile").initial_name(format!("{}.wooting.json", n.replace(' ', "-"))).build();
        dialog.save(win.as_ref(), gio::Cancellable::NONE, move |res| {
            let Ok(file) = res else { return };
            let Some(path) = file.path() else { return };
            let c = c.clone();
            spawn(async move {
                if let Some(v) = c.call("profile.export", json!({"slot": slot})).await {
                    match std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap_or_default()) {
                        Ok(()) => c.toast(format!("Exported to {}", path.display())),
                        Err(e) => c.toast(format!("Export failed: {e}")),
                    }
                }
            });
        });
    })));
    let c = ctx.clone();
    let w = widget.clone();
    let n = name.clone();
    group.add_action(&make("delete", Box::new(move || {
        let c = c.clone();
        let win = w.root().and_downcast::<gtk::Window>();
        let n = n.clone();
        spawn(async move {
            let dialog = adw::AlertDialog::new(Some("Delete profile"), Some(&format!("Remove “{n}” from the keyboard? This cannot be undone.")));
            dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete profile")]);
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            if dialog.choose_future(win.as_ref()).await == "delete" {
                if c.call("profile.delete", json!({"slot": slot, "confirm": true})).await.is_some() {
                    c.toast("Profile deleted");
                }
                c.refresh_bar().await;
                c.refresh_all();
            }
        });
    })));
    widget.insert_action_group("profile", Some(&group));
}

fn glib_variant_type() -> &'static gtk::glib::VariantTy {
    gtk::glib::VariantTy::INT32
}

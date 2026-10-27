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

    let (ob_card, ob) = card(
        "Onboard profiles",
        Some("Profiles stored on the keyboard (4 slots). Slot 1 is the default profile."),
    );
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_max_children_per_line(2);
    grid.set_min_children_per_line(2);
    grid.set_homogeneous(true);
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    ob.append(&grid);
    page.append(&ob_card);

    let (ln_card, ln) = card("Linked with an application", Some("Linked profiles activate automatically when their window is focused (Hyprland). Rules are stored in ~/.config/wooting/links.toml and handled by the wootd daemon."));
    let rules = gtk::ListBox::new();
    rules.add_css_class("boxed-list");
    rules.set_selection_mode(gtk::SelectionMode::None);
    ln.append(&rules);
    let add_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let class_entry = gtk::Entry::new();
    class_entry.set_placeholder_text(Some("Window class (e.g. cs2)"));
    class_entry.set_hexpand(true);
    let pick_window = gtk::MenuButton::builder().label("Open windows").build();
    let title_entry = gtk::Entry::new();
    title_entry.set_placeholder_text(Some("Title contains (optional)"));
    title_entry.set_hexpand(true);
    let profile_dd = gtk::DropDown::from_strings(&[]);
    let add_link = gtk::Button::with_label("Link");
    add_link.add_css_class("suggested-action");
    add_row.append(&class_entry);
    add_row.append(&pick_window);
    add_row.append(&title_entry);
    add_row.append(&profile_dd);
    add_row.append(&add_link);
    ln.append(&add_row);
    let def_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    def_row.append(&gtk::Label::new(Some("When no rule matches, use")));
    let default_dd = gtk::DropDown::from_strings(&["(keep current)"]);
    def_row.append(&default_dd);
    ln.append(&def_row);
    page.append(&ln_card);

    // Pick a class from the currently open windows (hyprctl clients).
    let ce = class_entry.clone();
    let pw = pick_window.clone();
    pick_window.connect_activate(move |_| {
        let _ = &pw;
        let _ = &ce;
    });
    let ce2 = class_entry.clone();
    let popover = gtk::Popover::new();
    let plist = gtk::ListBox::new();
    plist.add_css_class("boxed-list");
    popover.set_child(Some(&plist));
    pick_window.set_popover(Some(&popover));
    let pl2 = plist.clone();
    let pop2 = popover.clone();
    popover.connect_show(move |_| {
        while let Some(ch) = pl2.first_child() {
            pl2.remove(&ch);
        }
        let out = std::process::Command::new("hyprctl")
            .args(["clients", "-j"])
            .output();
        let Ok(out) = out else { return };
        let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) else {
            return;
        };
        let mut seen = std::collections::HashSet::new();
        for cl in v.as_array().into_iter().flatten() {
            let class = cl["class"].as_str().unwrap_or("").to_string();
            if class.is_empty() || !seen.insert(class.clone()) {
                continue;
            }
            let row = adw::ActionRow::builder()
                .title(&class)
                .subtitle(cl["title"].as_str().unwrap_or(""))
                .activatable(true)
                .build();
            let ce3 = ce2.clone();
            let pop3 = pop2.clone();
            row.connect_activated(move |_| {
                ce3.set_text(&class);
                pop3.popdown();
            });
            pl2.append(&row);
        }
    });

    let c = ctx.clone();
    let (rl, pdd, ddd) = (rules.clone(), profile_dd.clone(), default_dd.clone());
    let names: Rc<std::cell::RefCell<Vec<String>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
    let nm = names.clone();
    let load_links: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let (rl, pdd, ddd, nm) = (rl.clone(), pdd.clone(), ddd.clone(), nm.clone());
        spawn(async move {
            if let Some(list) = c.call("profile.list", json!({})).await {
                let n: Vec<String> = list
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p["name"].as_str().map(str::to_owned))
                    .collect();
                pdd.set_model(Some(&gtk::StringList::new(
                    &n.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                )));
                let mut d = vec!["(keep current)".to_string()];
                d.extend(n.iter().cloned());
                ddd.set_model(Some(&gtk::StringList::new(
                    &d.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                )));
                *nm.borrow_mut() = n;
            }
            while let Some(ch) = rl.first_child() {
                rl.remove(&ch);
            }
            let Ok(v) = c.backend.call("links.list", json!({})).await else {
                rl.append(
                    &adw::ActionRow::builder()
                        .title("Linked profiles need the wootd daemon")
                        .build(),
                );
                return;
            };
            if let Some(d) = v["default"].as_str() {
                let n = nm.borrow();
                if let Some(i) = n.iter().position(|x| x == d) {
                    ddd.set_selected(i as u32 + 1);
                }
            }
            let items = v["link"].as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                rl.append(
                    &adw::ActionRow::builder()
                        .title("No linked applications yet")
                        .build(),
                );
            }
            for (i, l) in items.iter().enumerate() {
                let what = match (l["class"].as_str(), l["title"].as_str()) {
                    (Some(c), Some(t)) => format!("{c} · title contains “{t}”"),
                    (Some(c), None) => c.to_string(),
                    (None, Some(t)) => format!("title contains “{t}”"),
                    _ => "?".to_string(),
                };
                let row = adw::ActionRow::builder()
                    .title(what)
                    .subtitle(format!("→ {}", l["profile"].as_str().unwrap_or("?")))
                    .build();
                let del = gtk::Button::from_icon_name("user-trash-symbolic");
                del.add_css_class("flat");
                del.set_valign(gtk::Align::Center);
                let c2 = c.clone();
                del.connect_clicked(move |_| {
                    let c2 = c2.clone();
                    spawn(async move {
                        c2.call("links.remove", json!({"index": i})).await;
                        c2.refresh_page();
                    });
                });
                row.add_suffix(&del);
                rl.append(&row);
            }
        });
    });
    let c = ctx.clone();
    let (ce, te, pdd, nm, ll) = (
        class_entry.clone(),
        title_entry.clone(),
        profile_dd.clone(),
        names.clone(),
        load_links.clone(),
    );
    add_link.connect_clicked(move |_| {
        let class = ce.text().trim().to_string();
        let title = te.text().trim().to_string();
        if class.is_empty() && title.is_empty() {
            c.toast("Enter a window class or a title fragment");
            return;
        }
        let profile = nm
            .borrow()
            .get(pdd.selected() as usize)
            .cloned()
            .unwrap_or_default();
        let mut p = json!({"profile": profile});
        if !class.is_empty() {
            p["class"] = class.into();
        }
        if !title.is_empty() {
            p["title"] = title.into();
        }
        let c = c.clone();
        let ll = ll.clone();
        spawn(async move {
            if c.call("links.add", p).await.is_some() {
                c.toast("Application linked");
            }
            ll();
        });
    });
    let c = ctx.clone();
    let nm = names.clone();
    default_dd.connect_selected_notify(move |dd| {
        let i = dd.selected() as usize;
        let profile = if i == 0 {
            None
        } else {
            nm.borrow().get(i - 1).cloned()
        };
        let c = c.clone();
        spawn(async move {
            c.call("links.default", json!({"profile": profile})).await;
        });
    });

    let c = ctx.clone();
    let g = grid.clone();
    let ll = load_links.clone();
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let g = g.clone();
        ll();
        spawn(async move {
            while let Some(ch) = g.first_child() {
                g.remove(&ch);
            }
            let Some(list) = c.call("profile.list", json!({})).await else {
                return;
            };
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
        let dialog = gtk::FileDialog::builder()
            .title("Import profile JSON")
            .build();
        dialog.open(win.as_ref(), gio::Cancellable::NONE, move |res| {
            let Ok(file) = res else { return };
            let Some(path) = file.path() else { return };
            spawn(async move {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    c.toast("Could not read file");
                    return;
                };
                let Ok(v) = serde_json::from_str::<Value>(&text) else {
                    c.toast("Not a JSON profile");
                    return;
                };
                let data = v.get("data").cloned().unwrap_or(v);
                let Some(list) = c.call("profile.list", json!({})).await else {
                    return;
                };
                let slot = list.as_array().map(|a| a.len()).unwrap_or(0);
                if slot >= 4 {
                    c.toast("All 4 slots are in use; delete one first");
                    return;
                }
                if c.call("profile.import", json!({"slot": slot, "data": data}))
                    .await
                    .is_some()
                {
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
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    swatch.add_css_class(&format!("pcard-{slot}"));
    bx.append(&swatch);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_margin_top(10);
    text.set_margin_bottom(10);
    text.set_hexpand(true);
    let t = gtk::Label::new(Some(&format!(
        "{}  {}{}",
        slot + 1,
        name,
        if dirty { " •" } else { "" }
    )));
    t.add_css_class("card-title");
    t.set_xalign(0.0);
    let s = gtk::Label::new(Some(&format!(
        "{}{}",
        if slot == 0 { "Default · " } else { "" },
        if active {
            "Active"
        } else {
            "Click ⋮ for actions"
        }
    )));
    s.add_css_class("dim-label");
    s.set_xalign(0.0);
    text.append(&t);
    text.append(&s);
    bx.append(&text);
    let menu = gio::Menu::new();
    menu.append(Some("Activate"), Some(&format!("profile.activate({slot})")));
    menu.append(Some("Rename…"), Some(&format!("profile.rename({slot})")));
    menu.append(
        Some("Duplicate"),
        Some(&format!("profile.duplicate({slot})")),
    );
    menu.append(
        Some("Set as default"),
        Some(&format!("profile.default({slot})")),
    );
    menu.append(Some("Export…"), Some(&format!("profile.export({slot})")));
    menu.append(Some("Delete…"), Some(&format!("profile.delete({slot})")));
    let mb = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .menu_model(&menu)
        .valign(gtk::Align::Center)
        .build();
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
    group.add_action(&make(
        "activate",
        Box::new(move || {
            let c = c.clone();
            spawn(async move {
                c.switch_profile(slot).await;
            });
        }),
    ));
    let c = ctx.clone();
    let w = widget.clone();
    let n = name.clone();
    group.add_action(&make(
        "rename",
        Box::new(move || {
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
                    c.call(
                        "profile.rename",
                        json!({"slot": slot, "name": entry.text().to_string()}),
                    )
                    .await;
                    c.refresh_bar().await;
                    c.refresh_page();
                }
            });
        }),
    ));
    let c = ctx.clone();
    let n = name.clone();
    group.add_action(&make(
        "duplicate",
        Box::new(move || {
            let c = c.clone();
            let n = n.clone();
            spawn(async move {
                if c.call(
                    "profile.create",
                    json!({"name": format!("{n} copy"), "from_slot": slot}),
                )
                .await
                .is_some()
                {
                    c.toast("Profile duplicated");
                }
                c.refresh_bar().await;
                c.refresh_page();
            });
        }),
    ));
    let c = ctx.clone();
    group.add_action(&make(
        "default",
        Box::new(move || {
            let c = c.clone();
            spawn(async move {
                if slot == 0 {
                    c.toast("Already the default profile");
                    return;
                }
                if c.call("profile.swap", json!({"a": 0, "b": slot}))
                    .await
                    .is_some()
                {
                    c.toast("Default profile changed (slots swapped)");
                }
                c.refresh_bar().await;
                c.refresh_all();
            });
        }),
    ));
    let c = ctx.clone();
    let w = widget.clone();
    let n = name.clone();
    group.add_action(&make(
        "export",
        Box::new(move || {
            let c = c.clone();
            let win = w.root().and_downcast::<gtk::Window>();
            let n = n.clone();
            let dialog = gtk::FileDialog::builder()
                .title("Export profile")
                .initial_name(format!("{}.wooting.json", n.replace(' ', "-")))
                .build();
            dialog.save(win.as_ref(), gio::Cancellable::NONE, move |res| {
                let Ok(file) = res else { return };
                let Some(path) = file.path() else { return };
                let c = c.clone();
                spawn(async move {
                    if let Some(v) = c.call("profile.export", json!({"slot": slot})).await {
                        match std::fs::write(
                            &path,
                            serde_json::to_string_pretty(&v).unwrap_or_default(),
                        ) {
                            Ok(()) => c.toast(format!("Exported to {}", path.display())),
                            Err(e) => c.toast(format!("Export failed: {e}")),
                        }
                    }
                });
            });
        }),
    ));
    let c = ctx.clone();
    let w = widget.clone();
    let n = name.clone();
    group.add_action(&make(
        "delete",
        Box::new(move || {
            let c = c.clone();
            let win = w.root().and_downcast::<gtk::Window>();
            let n = n.clone();
            spawn(async move {
                let dialog = adw::AlertDialog::new(
                    Some("Delete profile"),
                    Some(&format!(
                        "Remove “{n}” from the keyboard? This cannot be undone."
                    )),
                );
                dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete profile")]);
                dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
                if dialog.choose_future(win.as_ref()).await == "delete" {
                    if c.call("profile.delete", json!({"slot": slot, "confirm": true}))
                        .await
                        .is_some()
                    {
                        c.toast("Profile deleted");
                    }
                    c.refresh_bar().await;
                    c.refresh_all();
                }
            });
        }),
    ));
    widget.insert_action_group("profile", Some(&group));
}

fn glib_variant_type() -> &'static gtk::glib::VariantTy {
    gtk::glib::VariantTy::INT32
}

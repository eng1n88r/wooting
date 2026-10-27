//! Gamepad: Setup & Remap (mode, controller type, polling, button mapping),
//! Configuration (analog curve, joystick options) and a live Tester.

use crate::{
    app::{card, on_scale_settled, page_box, set_scale_quiet, Ctx},
    backend::spawn,
};
use adw::prelude::*;
use gtk::{cairo, glib};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};
use wooting_proto::gamepad::{BUTTONS, CURVE_PRESETS};

/// Curve points shared between the drawing area and the loaders.
type Curve = Rc<RefCell<Vec<(u32, u32)>>>;

pub fn build(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = gtk::Label::new(Some("Gamepad"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    head.append(&title);
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    head.append(&switcher);
    page.append(&head);
    page.append(&stack);

    let loading = Rc::new(Cell::new(false));

    // ----- Setup & Remap --------------------------------------------------
    let setup = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let (en_card, en) = card("Enable Gamepad", Some("Your Wooting keyboard can work just like a controller, sending out analog key signals. Changing these restarts the keyboard (gamepad mode and controller type change its USB identity); the app reconnects."));
    en_card.set_size_request(300, -1);
    let enable = adw::SwitchRow::builder().title("Gamepad mode").build();
    let ctype = adw::ComboRow::builder()
        .title("Controller type")
        .model(&gtk::StringList::new(&[
            "Xbox Controller",
            "Classic Controller",
        ]))
        .build();
    let polling = adw::ComboRow::builder()
        .title("Polling rate")
        .model(&gtk::StringList::new(&["1000 Hz", "250 Hz"]))
        .build();
    let g = adw::PreferencesGroup::new();
    g.add(&enable);
    g.add(&ctype);
    g.add(&polling);
    en.append(&g);
    let note = gtk::Label::new(Some(
        "These settings affect every profile that uses gamepad.",
    ));
    note.add_css_class("dim-label");
    note.set_xalign(0.0);
    note.set_wrap(true);
    en.append(&note);
    setup.append(&en_card);

    let (map_card, map) = card("Controller Mapping", Some("Select keys on the keyboard, then click a controller button to assign it. Right side: buttons currently mapped."));
    map_card.set_hexpand(true);
    let tiles = gtk::FlowBox::new();
    tiles.set_selection_mode(gtk::SelectionMode::None);
    tiles.set_max_children_per_line(13);
    tiles.set_min_children_per_line(6);
    tiles.set_column_spacing(4);
    tiles.set_row_spacing(4);
    tiles.set_homogeneous(true);
    tiles.set_halign(gtk::Align::Start);
    for (name, label, _) in BUTTONS {
        let b = gtk::Button::with_label(label);
        b.add_css_class("keytile");
        b.set_tooltip_text(Some(name));
        let c = ctx.clone();
        let n = name.to_string();
        b.connect_clicked(move |_| assign(&c, Some(n.clone())));
        tiles.insert(&b, -1);
    }
    map.append(&tiles);
    let tools = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let unmap = gtk::Button::with_label("Remove from selected keys");
    let c = ctx.clone();
    unmap.connect_clicked(move |_| assign(&c, None));
    tools.append(&unmap);
    let clear = gtk::Button::with_label("Clear all");
    clear.add_css_class("destructive-action");
    let c = ctx.clone();
    clear.connect_clicked(move |_| {
        let c = c.clone();
        spawn(async move {
            if let Some(v) = c.call("gamepad.get", json!({})).await {
                let keys: serde_json::Map<String, Value> = v["keys"]
                    .as_object()
                    .map(|m| m.keys().map(|k| (k.clone(), Value::Null)).collect())
                    .unwrap_or_default();
                if !keys.is_empty() {
                    c.edit("gamepad.set", json!({"keys": keys})).await;
                }
            }
            c.refresh_page();
        });
    });
    tools.append(&clear);
    let mapped = gtk::Label::new(None);
    mapped.add_css_class("dim-label");
    mapped.set_xalign(0.0);
    mapped.set_wrap(true);
    mapped.set_hexpand(true);
    tools.append(&mapped);
    map.append(&tools);
    let g2 = adw::PreferencesGroup::new();
    let kb_keys = adw::SwitchRow::builder().title("Enable keyboard keys").subtitle("Enable digital keys if you want to output keyboard keys for typing. Disable digital keys to prevent them from interfering with your controller output keys.").build();
    let override_row = adw::SwitchRow::builder()
        .title("Gamepad mapping override")
        .subtitle("Disable the keyboard input on keys with gamepad bindings on them.")
        .build();
    g2.add(&kb_keys);
    g2.add(&override_row);
    map.append(&g2);
    setup.append(&map_card);
    stack.add_titled(&setup, Some("setup"), "Setup & Remap");

    // ----- Configuration ----------------------------------------------------
    let config = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let (curve_card, cv) = card("Analog Curve", Some("How deep a key is pressed (0.10–4.00 mm) → joystick axis value (0–255). The first point's height is the anti-deadzone."));
    curve_card.set_size_request(360, -1);
    let curve: Curve = Rc::new(RefCell::new(Vec::new()));
    let area = gtk::DrawingArea::new();
    area.set_content_height(180);
    area.set_hexpand(true);
    let cvp = curve.clone();
    area.set_draw_func(move |_, cr, w, h| draw_curve(cr, w as f64, h as f64, &cvp.borrow()));
    cv.append(&area);
    let dz_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    dz_row.append(&gtk::Label::new(Some("Anti-deadzone")));
    let deadzone = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 255.0, 1.0);
    deadzone.set_hexpand(true);
    deadzone.set_draw_value(true);
    deadzone.set_value_pos(gtk::PositionType::Right);
    dz_row.append(&deadzone);
    cv.append(&dz_row);
    let pl = gtk::Label::new(Some("Select a preset"));
    pl.add_css_class("keygroup");
    pl.set_xalign(0.0);
    cv.append(&pl);
    let presets = gtk::FlowBox::new();
    presets.set_selection_mode(gtk::SelectionMode::None);
    presets.set_max_children_per_line(7);
    presets.set_column_spacing(4);
    presets.set_row_spacing(4);
    presets.set_homogeneous(true);
    presets.set_halign(gtk::Align::Start);
    let mut preset_buttons = Vec::new();
    for (name, _) in CURVE_PRESETS {
        let b = gtk::ToggleButton::with_label(&capitalize(name));
        b.add_css_class("keytile");
        let c = ctx.clone();
        let n = name.to_string();
        let ld = loading.clone();
        b.connect_clicked(move |b| {
            if ld.get() || !b.is_active() {
                return;
            }
            let c = c.clone();
            let n = n.clone();
            spawn(async move {
                c.edit("gamepad.curve.set", json!({"preset": n})).await;
                c.refresh_page();
            });
        });
        presets.insert(&b, -1);
        preset_buttons.push((name.to_string(), b));
    }
    let preset_buttons = Rc::new(preset_buttons);
    cv.append(&presets);
    config.append(&curve_card);

    let (resp_card, resp) = card("Gamepad Response", None);
    resp_card.set_hexpand(true);
    let g3 = adw::PreferencesGroup::new();
    let angle = adw::SwitchRow::builder().title("Enable angle adjustment").subtitle("When pressing forward and left/right you walk at a diagonal direction. Adjust the slider to modify the center point of the diagonal direction.").build();
    g3.add(&angle);
    let angle_row = adw::ActionRow::builder()
        .title("Adjust the diagonal angle")
        .build();
    let degrees = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 90.0, 1.0);
    degrees.set_size_request(220, -1);
    degrees.set_draw_value(true);
    degrees.set_value_pos(gtk::PositionType::Right);
    degrees.set_format_value_func(|_, v| format!("{v:.0}°"));
    degrees.set_valign(gtk::Align::Center);
    angle_row.add_suffix(&degrees);
    g3.add(&angle_row);
    let square = adw::SwitchRow::builder().title("Use Square joystick output").subtitle("Restrict the joystick movement vector to a square instead of a circle, allowing maximum axis output. Recommended for Rocket League and CS:GO; may break other games.").build();
    g3.add(&square);
    let snappy = adw::SwitchRow::builder().title("Snappy Joystick").subtitle("Use the maximum value of opposite sides of the axis rather than combining them: with left and right both pressed, whichever is deeper wins.").build();
    g3.add(&snappy);
    resp.append(&g3);
    config.append(&resp_card);
    stack.add_titled(&config, Some("config"), "Configuration");

    // ----- Tester -----------------------------------------------------------
    let tester = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let (t_card, tb) = card("Gamepad Tester", Some("Press keys: mapped controller buttons light up with their analog value, bars on the keyboard show the travel."));
    let live = adw::SwitchRow::builder()
        .title("Live analog read-out")
        .build();
    let g4 = adw::PreferencesGroup::new();
    g4.add(&live);
    tb.append(&g4);
    let active = gtk::Label::new(Some("Active: —"));
    active.set_xalign(0.0);
    active.set_wrap(true);
    tb.append(&active);
    tester.append(&t_card);
    stack.add_titled(&tester, Some("tester"), "Tester");

    // ----- Loading ------------------------------------------------------------
    let key_buttons: Rc<RefCell<HashMap<String, String>>> = Rc::new(RefCell::new(HashMap::new()));
    let labels: HashMap<&str, &str> = BUTTONS.iter().map(|(n, l, _)| (*n, *l)).collect();
    let labels = Rc::new(labels);
    let c = ctx.clone();
    let ld = loading.clone();
    let w = (
        enable.clone(),
        ctype.clone(),
        polling.clone(),
        kb_keys.clone(),
        override_row.clone(),
        mapped.clone(),
        angle.clone(),
        square.clone(),
        snappy.clone(),
    );
    let (cvl, areal, dzl, degl, pbl, kbl, lbl) = (
        curve.clone(),
        area.clone(),
        deadzone.clone(),
        degrees.clone(),
        preset_buttons.clone(),
        key_buttons.clone(),
        labels.clone(),
    );
    let handlers: Rc<RefCell<Option<(glib::SignalHandlerId, glib::SignalHandlerId)>>> =
        Rc::new(RefCell::new(None));
    let hl = handlers.clone();
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let ld = ld.clone();
        let w = w.clone();
        let (cvl, areal, dzl, degl, pbl, kbl, lbl, hl) = (
            cvl.clone(),
            areal.clone(),
            dzl.clone(),
            degl.clone(),
            pbl.clone(),
            kbl.clone(),
            lbl.clone(),
            hl.clone(),
        );
        spawn(async move {
            let Some(v) = c.call("gamepad.get", json!({})).await else {
                return;
            };
            ld.set(true);
            if let Some(d) = v["device"].as_object() {
                w.0.set_active(d["gamepad_mode"].as_bool().unwrap_or(false));
                w.1.set_selected(if d["controller"].as_str() == Some("classic") {
                    1
                } else {
                    0
                });
                w.2.set_selected(if d["polling_hz"].as_u64() == Some(250) {
                    1
                } else {
                    0
                });
            }
            w.3.set_active(v["keyboard_keys"].as_bool().unwrap_or(true));
            w.4.set_active(v["override"].as_bool().unwrap_or(false));
            w.6.set_active(v["angle_enabled"].as_bool().unwrap_or(false));
            w.7.set_active(v["square"].as_bool().unwrap_or(false));
            w.8.set_active(v["snappy"].as_bool().unwrap_or(true));
            let keys: HashMap<String, String> = v["keys"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, b)| (k.clone(), b.as_str().unwrap_or("?").to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let mut listed: Vec<String> = keys
                .iter()
                .map(|(k, b)| format!("{k} → {}", lbl.get(b.as_str()).copied().unwrap_or(b)))
                .collect();
            listed.sort();
            w.5.set_text(&if listed.is_empty() {
                "No buttons mapped".to_string()
            } else {
                listed.join("   ")
            });
            {
                let mut st = c.view.state.borrow_mut();
                st.badges = keys
                    .iter()
                    .map(|(k, b)| {
                        (
                            k.clone(),
                            lbl.get(b.as_str()).copied().unwrap_or(b).to_string(),
                        )
                    })
                    .collect();
            }
            c.view.area.queue_draw();
            *kbl.borrow_mut() = keys;
            let pts: Vec<(u32, u32)> = v["curve"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| {
                    (
                        p[0].as_u64().unwrap_or(0) as u32,
                        p[1].as_u64().unwrap_or(0) as u32,
                    )
                })
                .collect();
            *cvl.borrow_mut() = pts;
            areal.queue_draw();
            let preset = v["preset"].as_str().unwrap_or("");
            for (n, b) in pbl.iter() {
                b.set_active(n == preset);
            }
            if let Some((hd, hg)) = hl.borrow().as_ref() {
                set_scale_quiet(&dzl, hd, v["deadzone"].as_f64().unwrap_or(0.0));
                set_scale_quiet(&degl, hg, v["angle"].as_f64().unwrap_or(50.0) * 0.9);
            }
            degl.set_sensitive(v["angle_enabled"].as_bool().unwrap_or(false));
            ld.set(false);
        });
    });
    ctx.register_refresher("gamepad", load.clone());

    // ----- Handlers -----------------------------------------------------------
    let c = ctx.clone();
    let ld = loading.clone();
    enable.connect_active_notify(move |r| {
        if ld.get() {
            return;
        }
        device_set(&c, json!({"gamepad_mode": r.is_active()}));
    });
    let c = ctx.clone();
    let ld = loading.clone();
    ctype.connect_selected_notify(move |r| {
        if ld.get() {
            return;
        }
        device_set(
            &c,
            json!({"controller": if r.selected() == 1 { "classic" } else { "xbox" }}),
        );
    });
    let c = ctx.clone();
    let ld = loading.clone();
    polling.connect_selected_notify(move |r| {
        if ld.get() {
            return;
        }
        device_set(
            &c,
            json!({"polling_hz": if r.selected() == 1 { 250 } else { 1000 }}),
        );
    });
    for (row, field) in [
        (kb_keys.clone(), "keyboard_keys"),
        (override_row.clone(), "override"),
    ] {
        let c = ctx.clone();
        let ld = loading.clone();
        row.connect_active_notify(move |r| {
            if ld.get() {
                return;
            }
            let c = c.clone();
            let p = json!({ field: r.is_active() });
            spawn(async move {
                c.edit("gamepad.set", p).await;
                c.refresh_page();
            });
        });
    }
    for (row, field) in [
        (angle.clone(), "angle_enabled"),
        (square.clone(), "square"),
        (snappy.clone(), "snappy"),
    ] {
        let c = ctx.clone();
        let ld = loading.clone();
        row.connect_active_notify(move |r| {
            if ld.get() {
                return;
            }
            let c = c.clone();
            let p = json!({ field: r.is_active() });
            spawn(async move {
                c.edit("gamepad.curve.set", p).await;
                c.refresh_page();
            });
        });
    }
    let c = ctx.clone();
    let hd = on_scale_settled(&deadzone, move |v| {
        let c = c.clone();
        spawn(async move {
            c.edit("gamepad.curve.set", json!({"deadzone": v.round() as u32}))
                .await;
            c.refresh_page();
        });
    });
    let c = ctx.clone();
    let hg = on_scale_settled(&degrees, move |v| {
        let c = c.clone();
        let pct = (v / 0.9).round().clamp(0.0, 100.0) as u32;
        spawn(async move {
            c.edit("gamepad.curve.set", json!({"angle": pct})).await;
            c.refresh_page();
        });
    });
    *handlers.borrow_mut() = Some((hd, hg));

    // Tester polling.
    let running = Rc::new(Cell::new(false));
    let c = ctx.clone();
    let (act, kbl, lbl) = (active.clone(), key_buttons.clone(), labels.clone());
    live.connect_active_notify(move |sw| {
        running.set(sw.is_active());
        if !sw.is_active() {
            c.view.state.borrow_mut().analog.clear();
            c.view.area.queue_draw();
            act.set_text("Active: —");
            return;
        }
        let c = c.clone();
        let running = running.clone();
        let (act, kbl, lbl) = (act.clone(), kbl.clone(), lbl.clone());
        spawn(async move {
            while running.get() {
                if let Ok(v) = c.backend.call("device.analog", json!({})).await {
                    let mut st = c.view.state.borrow_mut();
                    st.analog.clear();
                    let mut names = Vec::new();
                    for (k, val) in v["pressed"].as_object().into_iter().flatten() {
                        let a = val.as_u64().unwrap_or(0) as u8;
                        st.analog.insert(k.clone(), a);
                        let kb = kbl.borrow();
                        match kb.get(k) {
                            Some(b) => names.push(format!(
                                "{} {}%",
                                lbl.get(b.as_str()).copied().unwrap_or(b),
                                (a as u32 * 100) / 255
                            )),
                            None => names.push(format!("{k} (no button)")),
                        }
                    }
                    drop(st);
                    c.view.area.queue_draw();
                    act.set_text(&if names.is_empty() {
                        "Active: —".to_string()
                    } else {
                        format!("Active: {}", names.join(", "))
                    });
                }
                glib::timeout_future(std::time::Duration::from_millis(80)).await;
            }
        });
    });
    if let Ok(tab) = std::env::var("WOOTING_GTK_GAMEPAD_TAB") {
        stack.set_visible_child_name(&tab);
    }
    page.upcast()
}

fn assign(c: &Ctx, button: Option<String>) {
    let keys = c.view.selected();
    if keys.is_empty() {
        c.toast("Select some keys on the keyboard first");
        return;
    }
    let map: serde_json::Map<String, Value> = keys
        .into_iter()
        .map(|k| (k, button.clone().map(Value::String).unwrap_or(Value::Null)))
        .collect();
    let c = c.clone();
    spawn(async move {
        c.edit("gamepad.set", json!({"keys": map})).await;
        c.refresh_page();
    });
}

fn device_set(c: &Ctx, p: Value) {
    let c = c.clone();
    spawn(async move {
        if c.call("settings.device.set", p).await.is_some() {
            c.toast("Keyboard restarting…");
        }
    });
}

fn capitalize(s: &str) -> String {
    let mut ch = s.chars();
    match ch.next() {
        Some(f) => f.to_uppercase().collect::<String>() + ch.as_str(),
        None => String::new(),
    }
}

fn draw_curve(cr: &cairo::Context, w: f64, h: f64, pts: &[(u32, u32)]) {
    let (l, t, r, b) = (34.0, 10.0, w - 10.0, h - 24.0);
    cr.set_source_rgba(0.5, 0.5, 0.5, 0.25);
    cr.set_line_width(1.0);
    for i in 0..=4 {
        let x = l + (r - l) * i as f64 / 4.0;
        cr.move_to(x, t);
        cr.line_to(x, b);
        let y = t + (b - t) * i as f64 / 4.0;
        cr.move_to(l, y);
        cr.line_to(r, y);
    }
    let _ = cr.stroke();
    cr.set_source_rgba(0.5, 0.5, 0.5, 0.9);
    cr.set_font_size(10.0);
    cr.move_to(l - 30.0, t + 8.0);
    let _ = cr.show_text("255");
    cr.move_to(l - 14.0, b);
    let _ = cr.show_text("0");
    cr.move_to(l, b + 14.0);
    let _ = cr.show_text("0.10 mm");
    cr.move_to(r - 44.0, b + 14.0);
    let _ = cr.show_text("4.00 mm");
    if pts.is_empty() {
        return;
    }
    let px = |p: &(u32, u32)| {
        (
            l + (r - l) * p.0 as f64 / 255.0,
            b - (b - t) * p.1 as f64 / 255.0,
        )
    };
    cr.set_source_rgba(0.98, 0.78, 0.2, 0.35);
    cr.move_to(l, b);
    for p in pts {
        let (x, y) = px(p);
        cr.line_to(x, y);
    }
    cr.line_to(r, b);
    cr.close_path();
    let _ = cr.fill();
    cr.set_source_rgb(0.98, 0.78, 0.2);
    cr.set_line_width(2.0);
    let (x0, y0) = px(&pts[0]);
    cr.move_to(x0, y0);
    for p in &pts[1..] {
        let (x, y) = px(p);
        cr.line_to(x, y);
    }
    let _ = cr.stroke();
    for p in pts {
        let (x, y) = px(p);
        cr.arc(x, y, 4.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
}

//! RGB Settings: stored colours, presets, brightness, base effect and
//! effect layers, plus live (direct) painting tools.

use crate::{
    app::{card, on_scale_settled, page_box, set_scale_quiet, Ctx},
    backend::spawn,
    keyboard_view::{frame_from_json, gdk_to_rgb, rgb_to_gdk},
};
use adw::prelude::*;
use gtk::{gdk, gio};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
use wooting_proto::Rgb;

/// Effect kinds: (field name, label, default params).
const BASE_EFFECTS: &[(&str, &str, Value)] = &[
    ("static", "Static", Value::Null),
    ("wave", "Wave", Value::Null),
    ("cycle", "Cycle", Value::Null),
    ("noise", "Noise", Value::Null),
    ("spiral", "Spiral", Value::Null),
];

fn default_layer(kind: &str) -> Value {
    match kind {
        "wave" => json!({"wave": {"look": 0, "speed": 16, "direction": 0}}),
        "cycle" => json!({"cycle": {"look": 0, "speed": 48}}),
        "noise" => json!({"noise": {"color1": 6175, "color2": 2017, "p3": 50, "p4": 25}}),
        "spiral" => json!({"spiral": {"color1": 63519, "color2": 2047, "spin_speed": 100, "spiral_speed": 139}}),
        "scan" => json!({"scan": {"width": 1, "speed": 15, "base_brightness": 30, "direction": 0}}),
        "breath" => json!({"breath": {"speed": 15, "p3": 0}}),
        "ripple" => json!({"ripple": {"p1": 0, "p2": 20, "p3": 30, "p4": 30}}),
        "trail" => json!({"trail": {"p1": 30, "p2": 4, "p3": 0, "p4": 65535}}),
        "touch" => json!({"touch": {"p1": 30, "p2": 0, "p3": 0, "p4": 0, "p5": 1}}),
        "jelly" => json!({"jelly": {"p1": 30, "p2": 0}}),
        "aoe" => json!({"aoe": {"p1": 30, "p2": 0}}),
        "mixing" => json!({"mixing": {"p1": 0}}),
        "rt_display" => json!({"rt_display": {"p1": 140}}),
        _ => json!({}),
    }
}

const LAYER_KINDS: &[(&str, &str)] = &[
    ("jelly", "Jelly"), ("scan", "Scan"), ("breath", "Breath"), ("ripple", "Ripple"), ("touch", "Touch"),
    ("aoe", "AOE"), ("mixing", "Mixing"), ("trail", "Trail"), ("rt_display", "RT display"),
];

fn kind_of(layer: &Value) -> String {
    layer.as_object().and_then(|m| m.keys().find(|k| !layer[k.as_str()].is_null()).cloned()).unwrap_or_else(|| "static".into())
}

/// Colour presets as column gradients (Wootility ships similar palettes).
const PRESETS: &[(&str, &[&str])] = &[
    ("Rainbow", &["#ff0000", "#ff8000", "#ffff00", "#00ff00", "#00ffff", "#0000ff", "#8000ff", "#ff00ff"]),
    ("Seawave", &["#003f5c", "#2f4b7c", "#1f77b4", "#00a6ed", "#00e5ff", "#7fffd4"]),
    ("Inferno", &["#000004", "#420a68", "#932667", "#dd513a", "#fca50a", "#fcffa4"]),
    ("Miami FPS", &["#ff00c8", "#ff5ac8", "#00e0ff", "#0080ff"]),
    ("Summer Breeze", &["#ffd166", "#06d6a0", "#118ab2", "#073b4c"]),
    ("Alternator", &["#ffffff", "#ff0000", "#ffffff", "#0000ff"]),
    ("Clay", &["#b5651d", "#d2a679", "#8b4513", "#deb887"]),
    ("Lekker", &["#ff6a00", "#ffffff", "#ff6a00"]),
    ("Sunset", &["#2b0a3d", "#7a1fa2", "#ff4e50", "#f9d423"]),
];

fn gradient_rows(stops: &[&str], layout: &wooting_proto::Layout) -> Vec<Vec<String>> {
    let cols: Vec<Rgb> = stops.iter().filter_map(|s| Rgb::parse_hex(s)).collect();
    let mut rows = vec![vec!["#000000".to_string(); 21]; 6];
    for key in &layout.keys {
        for (i, cell) in key.cells.iter().enumerate() {
            let (cx, _) = key.cell_center(i);
            let t = (cx / layout.width).clamp(0.0, 0.999) * (cols.len() - 1) as f32;
            let i0 = t.floor() as usize;
            let f = t - i0 as f32;
            let (a, b) = (cols[i0], cols[(i0 + 1).min(cols.len() - 1)]);
            let mix = |x: u8, y: u8| (x as f32 * (1.0 - f) + y as f32 * f).round() as u8;
            rows[cell[0] as usize][cell[1] as usize] = Rgb::new(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b)).to_hex();
        }
    }
    rows
}

pub fn build(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let title = gtk::Label::new(Some("RGB Settings"));
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    page.append(&title);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);

    // ----- colours card
    let (col_card, col) = card("Colours", Some("Pick a colour and apply it to the selected keys."));
    col_card.set_size_request(300, -1);
    let picker = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    picker.set_rgba(&rgb_to_gdk(Rgb::new(0x7a, 0xa2, 0xf7)));
    let hex = gtk::Entry::new();
    hex.set_placeholder_text(Some("#rrggbb"));
    hex.set_max_width_chars(9);
    let pick_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    pick_row.append(&picker);
    pick_row.append(&hex);
    col.append(&pick_row);
    let apply_sel = gtk::Button::with_label("Apply to selection");
    apply_sel.add_css_class("suggested-action");
    let fill = gtk::Button::with_label("Fill all keys");
    let ab = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    ab.append(&apply_sel);
    ab.append(&fill);
    col.append(&ab);
    let recent_label = gtk::Label::new(Some("Recently used"));
    recent_label.set_xalign(0.0);
    recent_label.add_css_class("dim-label");
    col.append(&recent_label);
    let recent = gtk::FlowBox::new();
    recent.set_selection_mode(gtk::SelectionMode::None);
    recent.set_max_children_per_line(8);
    col.append(&recent);
    row.append(&col_card);

    // ----- effects card
    let (fx_card, fx) = card("Effects", None);
    fx_card.set_hexpand(true);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let preset_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let pl = gtk::Label::new(Some("Presets"));
    pl.set_xalign(0.0);
    pl.add_css_class("dim-label");
    let names: Vec<&str> = std::iter::once("Pick a preset").chain(PRESETS.iter().map(|p| p.0)).collect();
    let presets = gtk::DropDown::from_strings(&names);
    preset_box.append(&pl);
    preset_box.append(&presets);
    top.append(&preset_box);
    let br_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    br_box.set_hexpand(true);
    let bl = gtk::Label::new(Some("Global brightness"));
    bl.set_xalign(0.0);
    bl.add_css_class("dim-label");
    let brightness = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    brightness.set_draw_value(true);
    brightness.set_value_pos(gtk::PositionType::Right);
    brightness.set_hexpand(true);
    br_box.append(&bl);
    br_box.append(&brightness);
    top.append(&br_box);
    fx.append(&top);

    let base_label = gtk::Label::new(Some("Base layer"));
    base_label.set_xalign(0.0);
    base_label.add_css_class("dim-label");
    fx.append(&base_label);
    let base_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let mut base_buttons = Vec::new();
    for (kind, label, _) in BASE_EFFECTS {
        let b = gtk::ToggleButton::with_label(label);
        base_row.append(&b);
        base_buttons.push((kind.to_string(), b));
    }
    fx.append(&base_row);
    let params = gtk::Box::new(gtk::Orientation::Vertical, 4);
    fx.append(&params);

    let layers_label = gtk::Label::new(Some("Effect layers"));
    layers_label.set_xalign(0.0);
    layers_label.add_css_class("dim-label");
    fx.append(&layers_label);
    let layers_list = gtk::ListBox::new();
    layers_list.add_css_class("boxed-list");
    layers_list.set_selection_mode(gtk::SelectionMode::None);
    fx.append(&layers_list);
    let add_kind = gtk::DropDown::from_strings(&LAYER_KINDS.iter().map(|k| k.1).collect::<Vec<_>>());
    let add_btn = gtk::Button::with_label("Add RGB Fx layer");
    let add_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    add_row.append(&add_kind);
    add_row.append(&add_btn);
    fx.append(&add_row);
    row.append(&fx_card);
    page.append(&row);

    // ----- live tools card
    let (live_card, live) = card("Live lighting (not stored)", Some("Paint the keyboard directly; use Store to keep the result as this profile's static colours."));
    let lb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let img_btn = gtk::Button::with_label("Paint from image…");
    let theme_btn = gtk::Button::with_label("Apply Omarchy theme");
    let store_btn = gtk::Button::with_label("Store live colours in profile");
    let release_btn = gtk::Button::with_label("Release");
    for b in [&img_btn, &theme_btn, &store_btn, &release_btn] {
        lb.append(b);
    }
    live.append(&lb);
    page.append(&live_card);

    // ----- state + loading
    let layers: Rc<RefCell<Vec<Value>>> = Rc::new(RefCell::new(Vec::new()));
    let recent_colors: Rc<RefCell<Vec<Rgb>>> = Rc::new(RefCell::new(Vec::new()));
    let loading = Rc::new(std::cell::Cell::new(false));
    let c = ctx.clone();
    let brightness_id = Rc::new(on_scale_settled(&brightness, move |v| {
        let c = c.clone();
        spawn(async move {
            c.edit("rgb.brightness.set", json!({"value": (v / 100.0 * 255.0).round() as u8})).await;
        });
    }));

    let c = ctx.clone();
    let (bs, bb, pr, ll, ld, lay, bid) = (brightness.clone(), base_buttons.clone(), params.clone(), layers_list.clone(), loading.clone(), layers.clone(), brightness_id.clone());
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let (bs, bb, pr, ll, ld, lay, bid) = (bs.clone(), bb.clone(), pr.clone(), ll.clone(), ld.clone(), lay.clone(), bid.clone());
        spawn(async move {
            if let Some(v) = c.call("rgb.profile.get", json!({})).await {
                c.view.set_frame(frame_from_json(&v));
            }
            let Some(fx) = c.call("rgb.effects.get", json!({})).await else { return };
            ld.set(true);
            set_scale_quiet(&bs, &bid, (fx["brightness"].as_f64().unwrap_or(255.0) / 255.0 * 100.0).round());
            let all: Vec<Value> = fx["layers"].as_array().cloned().unwrap_or_default();
            let base_kind = all.first().map(kind_of).unwrap_or_else(|| "static".into());
            for (k, b) in &bb {
                b.set_active(*k == base_kind);
            }
            *lay.borrow_mut() = all.clone();
            render_params(&c, &pr, &lay, 0);
            while let Some(ch) = ll.first_child() {
                ll.remove(&ch);
            }
            for (i, l) in all.iter().enumerate().skip(1) {
                let kind = kind_of(l);
                let r = adw::ActionRow::builder().title(LAYER_KINDS.iter().find(|k| k.0 == kind).map(|k| k.1).unwrap_or(&kind)).build();
                let del = gtk::Button::from_icon_name("user-trash-symbolic");
                del.add_css_class("flat");
                del.set_valign(gtk::Align::Center);
                let c2 = c.clone();
                let lay2 = lay.clone();
                del.connect_clicked(move |_| {
                    let mut v = lay2.borrow().clone();
                    v.remove(i);
                    let c2 = c2.clone();
                    spawn(async move {
                        c2.edit("rgb.effects.set", json!({"layers": v})).await;
                        c2.refresh_page();
                    });
                });
                r.add_suffix(&del);
                ll.append(&r);
            }
            ld.set(false);
        });
    });
    ctx.register_refresher("rgb", load.clone());

    // ----- colour actions
    let c = ctx.clone();
    let (p, rc, rf) = (picker.clone(), recent_colors.clone(), recent.clone());
    let apply_colour = Rc::new(move |colour: Rgb, all: bool| {
        let c = c.clone();
        {
            let mut r = rc.borrow_mut();
            r.retain(|x| *x != colour);
            r.insert(0, colour);
            r.truncate(12);
            while let Some(ch) = rf.first_child() {
                rf.remove(&ch);
            }
            for col in r.iter() {
                let b = gtk::Button::new();
                b.set_size_request(24, 24);
                let css = gtk::CssProvider::new();
                let cls = format!("sw-{}", col.to_hex().trim_start_matches('#'));
                css.load_from_string(&format!(".{cls} {{ background: {}; min-width: 20px; min-height: 20px; border-radius: 50%; }}", col.to_hex()));
                gtk::style_context_add_provider_for_display(&gdk::Display::default().unwrap(), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
                b.add_css_class(&cls);
                let p2 = p.clone();
                let col2 = *col;
                b.connect_clicked(move |_| p2.set_rgba(&rgb_to_gdk(col2)));
                rf.insert(&b, -1);
            }
        }
        spawn(async move {
            let sel = c.view.selected();
            if all || sel.is_empty() {
                let rows: Vec<Vec<String>> = vec![vec![colour.to_hex(); 21]; 6];
                c.edit("rgb.profile.set", json!({"rows": rows})).await;
            } else {
                let colors: serde_json::Map<String, Value> = sel.into_iter().map(|k| (k, Value::String(colour.to_hex()))).collect();
                c.edit("rgb.profile.set", json!({"colors": colors})).await;
            }
            c.refresh_page();
        });
    });
    let (p, h) = (picker.clone(), hex.clone());
    picker.connect_rgba_notify(move |pk| h.set_text(&gdk_to_rgb(&pk.rgba()).to_hex()));
    let p2 = p.clone();
    hex.connect_activate(move |e| {
        if let Some(c) = Rgb::parse_hex(&e.text()) {
            p2.set_rgba(&rgb_to_gdk(c));
        }
    });
    let (ac, p) = (apply_colour.clone(), picker.clone());
    apply_sel.connect_clicked(move |_| ac(gdk_to_rgb(&p.rgba()), false));
    let (ac, p) = (apply_colour.clone(), picker.clone());
    fill.connect_clicked(move |_| ac(gdk_to_rgb(&p.rgba()), true));

    // ----- presets / brightness / effects
    let c = ctx.clone();
    presets.connect_selected_notify(move |dd| {
        let i = dd.selected() as usize;
        if i == 0 {
            return;
        }
        let stops = PRESETS[i - 1].1;
        let c = c.clone();
        let dd = dd.clone();
        spawn(async move {
            let layout = c.view.state.borrow().layout.clone();
            if let Some(l) = layout {
                let rows = gradient_rows(stops, &l);
                c.edit("rgb.profile.set", json!({"rows": rows})).await;
            }
            dd.set_selected(0);
            c.refresh_page();
        });
    });
    for (kind, b) in &base_buttons {
        let c = ctx.clone();
        let kind = kind.clone();
        let lay = layers.clone();
        let ld = loading.clone();
        b.connect_clicked(move |btn| {
            if ld.get() || !btn.is_active() {
                return;
            }
            let mut v = lay.borrow().clone();
            let base = if kind == "static" { None } else { Some(default_layer(&kind)) };
            match (v.is_empty(), base) {
                (true, Some(b)) => v.insert(0, b),
                (false, Some(b)) => v[0] = b,
                (false, None) => { v.remove(0); }
                (true, None) => {}
            }
            let c = c.clone();
            spawn(async move {
                c.edit("rgb.effects.set", json!({"layers": v})).await;
                c.refresh_page();
            });
        });
    }
    let c = ctx.clone();
    let lay = layers.clone();
    let ak = add_kind.clone();
    add_btn.connect_clicked(move |_| {
        let kind = LAYER_KINDS[ak.selected() as usize].0;
        let mut v = lay.borrow().clone();
        if v.is_empty() {
            c.toast("Pick a base effect first (layers sit on top of it)");
            return;
        }
        v.push(default_layer(kind));
        let c = c.clone();
        spawn(async move {
            c.edit("rgb.effects.set", json!({"layers": v})).await;
            c.refresh_page();
        });
    });

    // ----- live tools
    let c = ctx.clone();
    img_btn.connect_clicked(move |b| {
        let filter = gtk::FileFilter::new();
        filter.add_mime_type("image/*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder().title("Choose an image").filters(&filters).build();
        let win = b.root().and_downcast::<gtk::Window>();
        let c = c.clone();
        dialog.open(win.as_ref(), gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    spawn(async move {
                        if let Some(v) = c.call("rgb.from_image", json!({"path": path})).await {
                            c.view.set_frame(frame_from_json(&v));
                        }
                    });
                }
            }
        });
    });
    let c = ctx.clone();
    theme_btn.connect_clicked(move |_| {
        let c = c.clone();
        spawn(async move {
            if c.call("theme.apply", json!({})).await.is_some() {
                if let Some(v) = c.call("rgb.get_frame", json!({})).await {
                    c.view.set_frame(frame_from_json(&v));
                }
            }
        });
    });
    let c = ctx.clone();
    store_btn.connect_clicked(move |_| {
        let c = c.clone();
        spawn(async move {
            let Some(v) = c.call("rgb.get_frame", json!({})).await else { return };
            c.edit("rgb.profile.set", json!({"rows": v["rows"]})).await;
            c.call("rgb.release", json!({})).await;
            c.toast("Live colours stored in the profile draft");
            c.refresh_page();
        });
    });
    let c = ctx.clone();
    release_btn.connect_clicked(move |_| {
        let c = c.clone();
        spawn(async move {
            c.call("rgb.release", json!({})).await;
            c.refresh_page();
        });
    });
    page.upcast()
}

/// Parameter editors for the base layer (index 0).
fn render_params(ctx: &Ctx, container: &gtk::Box, layers: &Rc<RefCell<Vec<Value>>>, index: usize) {
    while let Some(ch) = container.first_child() {
        container.remove(&ch);
    }
    let all = layers.borrow().clone();
    let Some(layer) = all.get(index) else { return };
    let kind = kind_of(layer);
    let Some(obj) = layer[&kind].as_object() else { return };
    let title = gtk::Label::new(Some("Effect parameters"));
    title.set_xalign(0.0);
    title.add_css_class("dim-label");
    container.append(&title);
    let grid = gtk::Grid::new();
    grid.set_column_spacing(12);
    grid.set_row_spacing(4);
    let mut keys: Vec<String> = obj.keys().cloned().collect();
    keys.sort();
    for (row, name) in keys.iter().enumerate() {
        let (label, max) = match name.as_str() {
            "look" => ("Look (0 rainbow …)", 10.0),
            "speed" => ("Speed", 100.0),
            "direction" => ("Direction", 3.0),
            "color1" | "color2" => ("Colour (RGB565)", 65535.0),
            "spin_speed" | "spiral_speed" => ("Speed", 255.0),
            "width" => ("Width (keys)", 20.0),
            "base_brightness" => ("Base brightness %", 100.0),
            _ => (name.as_str(), 65535.0),
        };
        let l = gtk::Label::new(Some(&format!("{label} [{name}]")));
        l.set_xalign(0.0);
        let spin = gtk::SpinButton::with_range(0.0, max, 1.0);
        spin.set_value(obj[name].as_f64().unwrap_or(0.0));
        let c = ctx.clone();
        let lay = layers.clone();
        let (kind2, name2) = (kind.clone(), name.clone());
        spin.connect_value_changed(move |s| {
            let mut v = lay.borrow().clone();
            v[index][&kind2][&name2] = json!(s.value() as u64);
            *lay.borrow_mut() = v.clone();
            let c = c.clone();
            spawn(async move { c.edit("rgb.effects.set", json!({"layers": v})).await; });
        });
        grid.attach(&l, 0, row as i32, 1, 1);
        grid.attach(&spin, 1, row as i32, 1, 1);
    }
    container.append(&grid);
}

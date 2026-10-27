//! Main window: Lighting, Keymap, Settings and Device pages around one
//! shared keyboard view.

use crate::{
    backend::{spawn, Backend},
    keyboard_view::{frame_from_json, gdk_to_rgb, layout_from_json, rgb_to_gdk, KeyboardView},
};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
use wooting_proto::Rgb;

#[derive(Clone)]
struct Ctx {
    backend: Backend,
    view: KeyboardView,
    toast: adw::ToastOverlay,
    status: gtk::Label,
}

impl Ctx {
    fn toast(&self, msg: impl Into<String>) {
        self.toast.add_toast(adw::Toast::new(&msg.into()));
    }

    /// Call the API, show errors as toasts, return the result.
    async fn call(&self, method: &'static str, params: Value) -> Option<Value> {
        match self.backend.call(method, params).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!(method, error = %e.message, "api error");
                self.toast(format!("{method}: {}", e.message));
                None
            }
        }
    }

    /// Show the live direct-mode frame, or the stored profile colours when
    /// the keyboard is driving its own LEDs.
    async fn refresh_frame(&self) {
        let direct = self
            .backend
            .call("device.info", json!({}))
            .await
            .map(|v| v["direct_rgb"].as_bool().unwrap_or(false))
            .unwrap_or(false);
        let method = if direct { "rgb.get_frame" } else { "rgb.profile.get" };
        if let Some(v) = self.call(method, json!({})).await {
            self.view.set_frame(frame_from_json(&v));
        }
    }

    async fn refresh_status(&self) {
        match self.backend.call("device.info", json!({})).await {
            Ok(v) => {
                let mode = if v["direct_rgb"].as_bool().unwrap_or(false) { "direct lighting" } else { "keyboard lighting" };
                self.status.set_text(&format!(
                    "{} · fw {} · profile {} · {} · via {}",
                    v["model"].as_str().unwrap_or("?"),
                    v["firmware"].as_str().unwrap_or("?"),
                    v["active_profile"],
                    mode,
                    if self.backend.is_daemon() { "wootd" } else { "direct HID" }
                ));
            }
            Err(e) => self.status.set_text(&format!("No keyboard: {}", e.message)),
        }
    }
}

pub fn build(app: &adw::Application) -> adw::ApplicationWindow {
    let backend = Backend::new();
    let view = KeyboardView::new();
    let toast = adw::ToastOverlay::new();
    let status = gtk::Label::new(Some("Connecting…"));
    status.add_css_class("dim-label");
    status.set_xalign(0.0);
    status.set_margin_start(12);
    status.set_margin_bottom(6);
    let ctx = Ctx { backend, view, toast: toast.clone(), status: status.clone() };

    let stack = adw::ViewStack::new();
    stack.add_titled_with_icon(&lighting_page(&ctx), Some("lighting"), "Lighting", "weather-clear-symbolic");
    stack.add_titled_with_icon(&keymap_page(&ctx), Some("keymap"), "Keymap", "input-keyboard-symbolic");
    stack.add_titled_with_icon(&actuation_page(&ctx), Some("actuation"), "Actuation", "object-flip-vertical-symbolic");
    stack.add_titled_with_icon(&settings_page(&ctx), Some("settings"), "Settings", "preferences-system-symbolic");
    stack.add_titled_with_icon(&device_page(&ctx), Some("device"), "Device", "drive-harddisk-symbolic");

    let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&switcher));

    // Keyboard view on top, page content below.
    let kb_frame = gtk::Frame::new(None);
    kb_frame.set_child(Some(&ctx.view.area));
    kb_frame.set_margin_start(12);
    kb_frame.set_margin_end(12);
    kb_frame.set_margin_top(12);
    install_drop_target(&ctx);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&kb_frame);
    content.append(&status);
    content.append(&stack);
    toast.set_child(Some(&content));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toast));

    let win = adw::ApplicationWindow::builder()
        .application(app)
        .title("Wooting")
        .default_width(960)
        .default_height(720)
        .content(&toolbar)
        .build();

    // Toggle code display with the page.
    let c = ctx.clone();
    stack.connect_visible_child_name_notify(move |s| {
        let codes = s.visible_child_name().as_deref() == Some("keymap");
        c.view.state.borrow_mut().show_codes = codes;
        c.view.area.queue_draw();
    });

    // `WOOTING_GTK_PAGE=keymap` opens on that page (handy for screenshots).
    if let Ok(page) = std::env::var("WOOTING_GTK_PAGE") {
        stack.set_visible_child_name(&page);
        ctx.view.state.borrow_mut().show_codes = page == "keymap";
    }

    let c = ctx.clone();
    spawn(async move {
        if let Some(v) = c.call("device.keys", json!({})).await {
            if let Some(l) = layout_from_json(&v) {
                c.view.set_layout(l);
            }
        }
        c.refresh_status().await;
        c.refresh_frame().await;
    });
    win
}

fn install_drop_target(ctx: &Ctx) {
    let target = gtk::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    let c = ctx.clone();
    target.connect_drop(move |_, value, _, _| {
        let Ok(file) = value.get::<gio::File>() else { return false };
        let Some(path) = file.path() else { return false };
        let c = c.clone();
        spawn(async move {
            if c.call("rgb.from_image", json!({"path": path})).await.is_some() {
                c.toast(format!("Painted from {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
            }
            c.refresh_frame().await;
            c.refresh_status().await;
        });
        true
    });
    ctx.view.area.add_controller(target);
}

fn page_box() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 12);
    b.set_margin_start(12);
    b.set_margin_end(12);
    b.set_margin_top(6);
    b.set_margin_bottom(12);
    b
}

fn lighting_page(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let group = adw::PreferencesGroup::builder().title("Direct lighting").description("Click keys to select (Shift-click for several). Changes are live and volatile until you save them to the keyboard.").build();

    // Colour row: picker + apply to selection / all.
    let dialog = gtk::ColorDialog::new();
    let picker = gtk::ColorDialogButton::new(Some(dialog));
    picker.set_rgba(&rgb_to_gdk(Rgb::new(0x7a, 0xa2, 0xf7)));
    picker.set_valign(gtk::Align::Center);
    let apply_sel = gtk::Button::with_label("Colour selected");
    apply_sel.set_valign(gtk::Align::Center);
    let fill = gtk::Button::with_label("Fill all");
    fill.set_valign(gtk::Align::Center);
    let row = adw::ActionRow::builder().title("Colour").build();
    row.add_suffix(&picker);
    row.add_suffix(&apply_sel);
    row.add_suffix(&fill);
    group.add(&row);

    let c = ctx.clone();
    let p = picker.clone();
    apply_sel.connect_clicked(move |_| {
        let keys = c.view.selected();
        if keys.is_empty() {
            c.toast("Select some keys first");
            return;
        }
        let colour = gdk_to_rgb(&p.rgba()).to_hex();
        let colors: serde_json::Map<String, Value> = keys.into_iter().map(|k| (k, Value::String(colour.clone()))).collect();
        let c = c.clone();
        spawn(async move {
            c.call("rgb.set_keys", json!({"colors": colors})).await;
            c.refresh_frame().await;
            c.refresh_status().await;
        });
    });
    let c = ctx.clone();
    let p = picker.clone();
    fill.connect_clicked(move |_| {
        let colour = gdk_to_rgb(&p.rgba()).to_hex();
        let c = c.clone();
        spawn(async move {
            c.call("rgb.fill", json!({"color": colour})).await;
            c.refresh_frame().await;
            c.refresh_status().await;
        });
    });

    // Image row.
    let img_btn = gtk::Button::with_label("Choose image…");
    img_btn.set_valign(gtk::Align::Center);
    let mode = gtk::DropDown::from_strings(&["cover", "fit", "stretch"]);
    mode.set_valign(gtk::Align::Center);
    let img_row = adw::ActionRow::builder().title("Paint from image").subtitle("Or drop an image file onto the keyboard").build();
    img_row.add_suffix(&mode);
    img_row.add_suffix(&img_btn);
    group.add(&img_row);
    let c = ctx.clone();
    let m = mode.clone();
    img_btn.connect_clicked(move |b| {
        let filter = gtk::FileFilter::new();
        filter.add_mime_type("image/*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder().title("Choose an image").filters(&filters).build();
        let win = b.root().and_downcast::<gtk::Window>();
        let c = c.clone();
        let mode = ["cover", "fit", "stretch"][m.selected() as usize].to_string();
        dialog.open(win.as_ref(), gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    spawn(async move {
                        c.call("rgb.from_image", json!({"path": path, "mode": mode})).await;
                        c.refresh_frame().await;
                        c.refresh_status().await;
                    });
                }
            }
        });
    });

    // Theme row.
    let theme_btn = gtk::Button::with_label("Apply Omarchy theme");
    theme_btn.set_valign(gtk::Align::Center);
    let theme_row = adw::ActionRow::builder().title("Omarchy theme").subtitle("Colours from the current theme's colors.toml").build();
    theme_row.add_suffix(&theme_btn);
    group.add(&theme_row);
    let c = ctx.clone();
    theme_btn.connect_clicked(move |_| {
        let c = c.clone();
        spawn(async move {
            c.call("theme.apply", json!({})).await;
            c.refresh_frame().await;
            c.refresh_status().await;
        });
    });
    page.append(&group);

    // Persist / release.
    let g2 = adw::PreferencesGroup::builder().title("Keyboard memory").build();
    let save = gtk::Button::with_label("Save to active profile");
    save.add_css_class("suggested-action");
    save.set_valign(gtk::Align::Center);
    let release = gtk::Button::with_label("Release");
    release.set_valign(gtk::Align::Center);
    let r = adw::ActionRow::builder().title("Persist current colours").subtitle("Writes the keyboard's flash so the colours survive unplugging. Release hands the LEDs back to the keyboard.").build();
    r.add_suffix(&release);
    r.add_suffix(&save);
    g2.add(&r);
    let c = ctx.clone();
    save.connect_clicked(move |_| {
        let c = c.clone();
        spawn(async move {
            if c.call("rgb.save", json!({})).await.is_some() {
                c.toast("Saved to keyboard");
            }
        });
    });
    let c = ctx.clone();
    release.connect_clicked(move |_| {
        let c = c.clone();
        spawn(async move {
            c.call("rgb.release", json!({})).await;
            c.refresh_status().await;
            c.refresh_frame().await;
        });
    });

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 255.0, 1.0);
    scale.set_hexpand(true);
    scale.set_draw_value(true);
    scale.set_value_pos(gtk::PositionType::Right);
    let br = adw::ActionRow::builder().title("Stored brightness").subtitle("Applied when the slider is released").build();
    br.add_suffix(&scale);
    g2.add(&br);
    let c = ctx.clone();
    let s = scale.clone();
    spawn(async move {
        if let Some(v) = c.call("rgb.brightness.get", json!({})).await {
            s.set_value(v["value"].as_f64().unwrap_or(0.0));
        }
    });
    let c = ctx.clone();
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    scale.connect_value_changed(move |s| {
        let value = s.value().round() as u8;
        let c = c.clone();
        if let Some(id) = pending.borrow_mut().take() {
            id.remove();
        }
        let p = pending.clone();
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
            p.borrow_mut().take();
            spawn(async move {
                c.call("rgb.brightness.set", json!({"value": value})).await;
            });
        });
        *pending.borrow_mut() = Some(id);
    });
    page.append(&g2);
    page.upcast()
}

fn keymap_page(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let group = adw::PreferencesGroup::builder()
        .title("Key map")
        .description("Select keys on the keyboard, pick what they should send, then Apply (live) or Save (also written to the active profile's flash). 'none' disables a key.")
        .build();

    let layer = gtk::DropDown::from_strings(&["Main layer", "Fn layer 1", "Fn layer 2"]);
    layer.set_valign(gtk::Align::Center);
    let layer_row = adw::ActionRow::builder().title("Layer").build();
    layer_row.add_suffix(&layer);
    group.add(&layer_row);

    let names: Vec<&str> = wooting_proto::keycode::KEYCODES.iter().map(|(_, n)| *n).collect();
    let code = gtk::DropDown::from_strings(&names);
    code.set_enable_search(true);
    code.set_valign(gtk::Align::Center);
    let apply = gtk::Button::with_label("Apply");
    apply.set_valign(gtk::Align::Center);
    let save = gtk::Button::with_label("Apply & Save");
    save.add_css_class("suggested-action");
    save.set_valign(gtk::Align::Center);
    let code_row = adw::ActionRow::builder().title("Selected keys send").build();
    code_row.add_suffix(&code);
    code_row.add_suffix(&apply);
    code_row.add_suffix(&save);
    group.add(&code_row);

    let reset = gtk::Button::with_label("Reset layer");
    reset.set_valign(gtk::Align::Center);
    let reset_row = adw::ActionRow::builder().title("Factory mapping").subtitle("Restores this layer's default mapping and saves it").build();
    reset_row.add_suffix(&reset);
    group.add(&reset_row);
    page.append(&group);

    let c = ctx.clone();
    let l = layer.clone();
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let layer = l.selected() as u8;
        spawn(async move {
            let Some(v) = c.call("keymap.get", json!({"layer": layer})).await else { return };
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
    let l = load.clone();
    layer.connect_selected_notify(move |_| l());
    if let Some(n) = std::env::var("WOOTING_GTK_LAYER").ok().and_then(|v| v.parse::<u32>().ok()) {
        layer.set_selected(n);
    }

    let names: Rc<Vec<String>> = Rc::new(names.iter().map(|n| n.to_string()).collect());
    let make_apply = |persist: bool| {
        let c = ctx.clone();
        let layer = layer.clone();
        let code = code.clone();
        let load = load.clone();
        let names = names.clone();
        move |_: &gtk::Button| {
            let keys = c.view.selected();
            if keys.is_empty() {
                c.toast("Select some keys first");
                return;
            }
            let name = names[code.selected() as usize].to_string();
            let map: serde_json::Map<String, Value> = keys.into_iter().map(|k| (k, Value::String(name.clone()))).collect();
            let c = c.clone();
            let load = load.clone();
            let layer = layer.selected() as u8;
            spawn(async move {
                if c.call("keymap.set", json!({"keys": map, "layer": layer, "persist": persist})).await.is_some() {
                    c.toast(if persist { "Mapping saved to keyboard" } else { "Mapping applied (not saved)" });
                }
                load();
            });
        }
    };
    apply.connect_clicked(make_apply(false));
    save.connect_clicked(make_apply(true));
    let c = ctx.clone();
    let l = layer.clone();
    let ld = load.clone();
    reset.connect_clicked(move |_| {
        let c = c.clone();
        let ld = ld.clone();
        let layer = l.selected() as u8;
        spawn(async move {
            if c.call("keymap.reset", json!({"layer": layer, "persist": true})).await.is_some() {
                c.toast("Layer reset to factory mapping");
            }
            ld();
        });
    });
    load();
    page.upcast()
}

fn actuation_page(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let group = adw::PreferencesGroup::builder()
        .title("Actuation &amp; rapid trigger")
        .description("Applies to every key of the active profile. Apply is live; Save also writes the profile's flash.")
        .build();

    let scale = |min: f64, max: f64| {
        let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, 0.05);
        s.set_hexpand(true);
        s.set_draw_value(true);
        s.set_digits(2);
        s.set_value_pos(gtk::PositionType::Right);
        s.set_size_request(320, -1);
        s
    };
    let actuation = scale(0.1, 4.0);
    let r1 = adw::ActionRow::builder().title("Actuation point").subtitle("mm of travel before a key registers").build();
    r1.add_suffix(&actuation);
    group.add(&r1);

    let rt = gtk::Switch::new();
    rt.set_valign(gtk::Align::Center);
    let r2 = adw::ActionRow::builder().title("Rapid trigger").subtitle("Re-arm a key as soon as it moves back up").build();
    r2.add_suffix(&rt);
    group.add(&r2);

    let press = scale(0.1, 4.0);
    let r3 = adw::ActionRow::builder().title("Sensitivity (press)").build();
    r3.add_suffix(&press);
    group.add(&r3);

    let split = gtk::Switch::new();
    split.set_valign(gtk::Align::Center);
    let r4 = adw::ActionRow::builder().title("Split sensitivity").subtitle("Separate release sensitivity").build();
    r4.add_suffix(&split);
    group.add(&r4);

    let release = scale(0.1, 4.0);
    let r5 = adw::ActionRow::builder().title("Sensitivity (release)").build();
    r5.add_suffix(&release);
    group.add(&r5);

    let tachyon = gtk::Switch::new();
    tachyon.set_valign(gtk::Align::Center);
    let r6 = adw::ActionRow::builder().title("Tachyon mode").subtitle("Fastest scanning; limits brightness and disables RGB effects").build();
    r6.add_suffix(&tachyon);
    group.add(&r6);

    let apply = gtk::Button::with_label("Apply");
    apply.set_valign(gtk::Align::Center);
    let save = gtk::Button::with_label("Apply & Save");
    save.add_css_class("suggested-action");
    save.set_valign(gtk::Align::Center);
    let r7 = adw::ActionRow::builder().title("Write to keyboard").build();
    r7.add_suffix(&apply);
    r7.add_suffix(&save);
    group.add(&r7);
    page.append(&group);

    let widgets = (actuation.clone(), rt.clone(), press.clone(), split.clone(), release.clone(), tachyon.clone());
    let c = ctx.clone();
    let w = widgets.clone();
    let load: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let w = w.clone();
        spawn(async move {
            let Some(v) = c.call("settings.get", json!({})).await else { return };
            w.0.set_value(v["actuation_mm"].as_f64().unwrap_or(1.6));
            w.1.set_active(v["rapid_trigger"].as_bool().unwrap_or(false));
            w.2.set_value(v["rt_press_mm"].as_f64().unwrap_or(0.4));
            let rel = v["rt_release_mm"].as_f64();
            w.3.set_active(rel.is_some());
            w.4.set_value(rel.unwrap_or(v["rt_press_mm"].as_f64().unwrap_or(0.4)));
            w.4.set_sensitive(rel.is_some());
            w.5.set_active(v["tachyon"].as_bool().unwrap_or(false));
        });
    });
    let rel = release.clone();
    split.connect_active_notify(move |s| rel.set_sensitive(s.is_active()));
    let make_apply = |persist: bool| {
        let c = ctx.clone();
        let w = widgets.clone();
        let load = load.clone();
        move |_: &gtk::Button| {
            let params = json!({
                "actuation_mm": w.0.value(),
                "rapid_trigger": w.1.is_active(),
                "rt_press_mm": w.2.value(),
                "rt_release_mm": if w.3.is_active() { Value::from(w.4.value()) } else { Value::Null },
                "tachyon": w.5.is_active(),
                "persist": persist,
            });
            let c = c.clone();
            let load = load.clone();
            spawn(async move {
                if c.call("settings.set", params).await.is_some() {
                    c.toast(if persist { "Settings saved to keyboard" } else { "Settings applied (not saved)" });
                }
                load();
            });
        }
    };
    apply.connect_clicked(make_apply(false));
    save.connect_clicked(make_apply(true));
    load();
    page.upcast()
}

fn settings_page(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let group = adw::PreferencesGroup::builder().title("Lighting power saving").description("Device-wide. 0 = never.").build();
    let dim = adw::SpinRow::with_range(0.0, 240.0, 1.0);
    dim.set_title("Dim lighting after (minutes)");
    let off = adw::SpinRow::with_range(0.0, 240.0, 1.0);
    off.set_title("Turn lighting off after (minutes)");
    group.add(&dim);
    group.add(&off);
    let apply = gtk::Button::with_label("Save");
    apply.add_css_class("suggested-action");
    apply.set_valign(gtk::Align::Center);
    let r = adw::ActionRow::builder().title("Write to keyboard").build();
    r.add_suffix(&apply);
    group.add(&r);
    page.append(&group);
    let c = ctx.clone();
    let (d, o) = (dim.clone(), off.clone());
    spawn(async move {
        if let Some(v) = c.call("settings.global.get", json!({})).await {
            let ps = &v["power_saving"];
            d.set_value(ps["dim_after_minutes"].as_f64().unwrap_or(0.0));
            o.set_value(ps["off_after_minutes"].as_f64().unwrap_or(0.0));
        }
    });
    let c = ctx.clone();
    apply.connect_clicked(move |_| {
        let c = c.clone();
        let params = json!({"dim_after_minutes": dim.value() as u64, "off_after_minutes": off.value() as u64});
        spawn(async move {
            if c.call("settings.global.set", params).await.is_some() {
                c.toast("Power saving saved");
            }
        });
    });
    page.upcast()
}

fn device_page(ctx: &Ctx) -> gtk::Widget {
    let page = page_box();
    let info = adw::PreferencesGroup::builder().title("Device").build();
    let rows: Vec<(&str, adw::ActionRow)> = ["model", "firmware", "serial", "path", "protocol"]
        .into_iter()
        .map(|k| (k, adw::ActionRow::builder().title(k).build()))
        .collect();
    for (_, r) in &rows {
        info.add(r);
    }
    page.append(&info);

    let profiles = adw::PreferencesGroup::builder().title("Profiles").description("Click to activate").build();
    let listbox = gtk::ListBox::new();
    listbox.add_css_class("boxed-list");
    listbox.set_selection_mode(gtk::SelectionMode::None);
    profiles.add(&listbox);
    page.append(&profiles);

    let rename = adw::EntryRow::builder().title("Rename active profile").show_apply_button(true).build();
    profiles.add(&rename);
    let c = ctx.clone();
    rename.connect_apply(move |e| {
        let name = e.text().to_string();
        if name.trim().is_empty() {
            return;
        }
        let c = c.clone();
        spawn(async move {
            if let Some(v) = c.call("device.info", json!({})).await {
                let slot = v["active_profile"].as_u64().unwrap_or(0);
                if c.call("profile.rename", json!({"slot": slot, "name": name})).await.is_some() {
                    c.toast("Profile renamed");
                }
            }
            c.refresh_status().await;
        });
    });

    let tools = adw::PreferencesGroup::builder().title("Tools").build();
    let backup = gtk::Button::with_label("Create backup");
    backup.set_valign(gtk::Align::Center);
    let br = adw::ActionRow::builder().title("Backup").subtitle("Dump every readable setting to ~/.local/share/wooting/backups").build();
    br.add_suffix(&backup);
    tools.add(&br);
    page.append(&tools);
    let c = ctx.clone();
    backup.connect_clicked(move |_| {
        let c = c.clone();
        spawn(async move {
            if let Some(v) = c.call("backup.create", json!({})).await {
                c.toast(format!("Backup written to {}", v["path"].as_str().unwrap_or("?")));
            }
        });
    });

    let c = ctx.clone();
    let lb = listbox.clone();
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        let c = c.clone();
        let lb = lb.clone();
        let rows = rows.clone();
        spawn(async move {
            if let Some(v) = c.call("device.info", json!({})).await {
                for (k, r) in &rows {
                    r.set_subtitle(&v[k].as_str().map(str::to_owned).unwrap_or_else(|| v[k].to_string()));
                }
            }
            while let Some(child) = lb.first_child() {
                lb.remove(&child);
            }
            let Some(list) = c.call("profile.list", json!({})).await else { return };
            for p in list.as_array().into_iter().flatten() {
                let slot = p["slot"].as_u64().unwrap_or(0) as u8;
                let active = p["active"].as_bool().unwrap_or(false);
                let row = adw::ActionRow::builder()
                    .title(p["name"].as_str().unwrap_or("?"))
                    .subtitle(format!("slot {slot}{}", if active { " · active" } else { "" }))
                    .activatable(true)
                    .build();
                let swatch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                swatch.set_size_request(14, 14);
                swatch.set_valign(gtk::Align::Center);
                let colour = p["color"].clone();
                let css = gtk::CssProvider::new();
                css.load_from_string(&format!(
                    ".swatch{slot} {{ background: rgb({},{},{}); border-radius: 7px; }}",
                    colour["r"], colour["g"], colour["b"]
                ));
                gtk::style_context_add_provider_for_display(&gdk::Display::default().unwrap(), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
                swatch.add_css_class(&format!("swatch{slot}"));
                row.add_prefix(&swatch);
                if active {
                    row.add_suffix(&gtk::Image::from_icon_name("object-select-symbolic"));
                }
                let c2 = c.clone();
                row.connect_activated(move |_| {
                    let c2 = c2.clone();
                    spawn(async move {
                        if c2.call("profile.activate", json!({"slot": slot})).await.is_some() {
                            c2.toast(format!("Activated profile {slot}"));
                        }
                        c2.refresh_status().await;
                    });
                });
                lb.append(&row);
            }
        });
    });
    refresh();
    // Re-read after a profile activation elsewhere: cheap periodic refresh.
    let r = refresh.clone();
    glib::timeout_add_seconds_local(15, move || {
        r();
        glib::ControlFlow::Continue
    });
    page.upcast()
}

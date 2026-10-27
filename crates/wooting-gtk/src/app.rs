//! Application shell: sidebar navigation, profile bar, keyboard view and
//! the shared context every page uses.

use crate::{
    backend::{spawn, Backend},
    keyboard_view::{frame_from_json, layout_from_json, KeyboardView, Overlay},
    pages,
};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

/// Page refreshers by page id.
type Refreshers = Rc<RefCell<HashMap<String, Rc<dyn Fn()>>>>;

/// Shared state handed to every page.
#[derive(Clone)]
pub struct Ctx {
    pub backend: Backend,
    pub view: KeyboardView,
    pub toast: adw::ToastOverlay,
    pub bar: ProfileBar,
    /// Page refreshers by page id; the shell calls the visible one after
    /// profile switches, undo/redo, save and discard.
    refreshers: Refreshers,
    current_page: Rc<RefCell<String>>,
    /// Selection count label + buttons shown under the keyboard.
    selection_label: gtk::Label,
}

#[derive(Clone)]
pub struct ProfileBar {
    pub swatch: gtk::Box,
    pub name: gtk::Label,
    pub menu: gtk::MenuButton,
    pub undo: gtk::Button,
    pub redo: gtk::Button,
    pub save: gtk::Button,
    pub discard: gtk::Button,
    pub popover_list: gtk::ListBox,
}

impl Ctx {
    pub fn toast(&self, msg: impl Into<String>) {
        self.toast.add_toast(adw::Toast::new(&msg.into()));
    }

    /// Call the API, show errors as toasts, return the result.
    pub async fn call(&self, method: &'static str, params: Value) -> Option<Value> {
        match self.backend.call(method, params).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!(method, error = %e.message, "api error");
                self.toast(format!("{method}: {}", e.message));
                None
            }
        }
    }

    /// Draft edit: live only; the profile bar's Save commits.
    pub async fn edit(&self, method: &'static str, mut params: Value) -> Option<Value> {
        if let Value::Object(m) = &mut params {
            m.insert("persist".into(), Value::Bool(false));
        }
        let r = self.call(method, params).await;
        self.refresh_bar().await;
        r
    }

    pub fn register_refresher(&self, page: &str, f: Rc<dyn Fn()>) {
        self.refreshers.borrow_mut().insert(page.to_string(), f);
    }

    /// Re-run the visible page's loader (after undo/redo/save/switch).
    pub fn refresh_page(&self) {
        let page = self.current_page.borrow().clone();
        if let Some(f) = self.refreshers.borrow().get(&page).cloned() {
            f();
        }
    }

    pub fn refresh_all(&self) {
        let fs: Vec<Rc<dyn Fn()>> = self.refreshers.borrow().values().cloned().collect();
        for f in fs {
            f();
        }
    }

    /// Update the profile bar (name, badge colour, dirty/undo/redo state).
    pub async fn refresh_bar(&self) {
        let Some(info) = self.call("device.info", json!({})).await else {
            return;
        };
        let draft = &info["draft"];
        let dirty = draft["dirty"].as_bool().unwrap_or(false);
        self.bar.save.set_sensitive(dirty);
        self.bar.discard.set_sensitive(dirty);
        self.bar
            .undo
            .set_sensitive(draft["can_undo"].as_bool().unwrap_or(false));
        self.bar
            .redo
            .set_sensitive(draft["can_redo"].as_bool().unwrap_or(false));
        let active = info["active_profile"].as_u64();
        if let Some(list) = self.call("profile.list", json!({})).await {
            while let Some(c) = self.bar.popover_list.first_child() {
                self.bar.popover_list.remove(&c);
            }
            for p in list.as_array().into_iter().flatten() {
                let slot = p["slot"].as_u64().unwrap_or(0);
                let is_active = Some(slot) == active;
                let name = p["name"].as_str().unwrap_or("?").to_string();
                if is_active {
                    self.bar
                        .name
                        .set_text(&format!("{name}{}", if dirty { " •" } else { "" }));
                    set_swatch(&self.bar.swatch, &p["color"], slot);
                }
                let row = adw::ActionRow::builder()
                    .title(format!("{}  {}", slot + 1, name))
                    .subtitle(if slot == 0 { "Default" } else { "" })
                    .activatable(!is_active)
                    .build();
                if is_active {
                    row.add_suffix(&gtk::Image::from_icon_name("object-select-symbolic"));
                }
                let c = self.clone();
                row.connect_activated(move |_| {
                    let c = c.clone();
                    c.bar.menu.popdown();
                    spawn(async move { c.switch_profile(slot as u8).await });
                });
                self.bar.popover_list.append(&row);
            }
        }
    }

    /// Switch profile, asking what to do with unsaved edits.
    pub async fn switch_profile(&self, slot: u8) {
        let info = self.call("device.info", json!({})).await;
        let dirty = info
            .as_ref()
            .map(|i| i["draft"]["dirty"].as_bool().unwrap_or(false))
            .unwrap_or(false);
        let mut params = json!({"slot": slot});
        if dirty {
            let dialog = adw::AlertDialog::new(
                Some("Unsaved changes"),
                Some("The current profile has edits that are not saved to the keyboard."),
            );
            dialog.add_responses(&[
                ("cancel", "Cancel"),
                ("discard", "Discard"),
                ("save", "Save and switch"),
            ]);
            dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
            dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
            let win = self.view.area.root().and_downcast::<gtk::Window>();
            let resp = dialog.choose_future(win.as_ref()).await;
            match resp.as_str() {
                "discard" => params["discard"] = true.into(),
                "save" => params["save"] = true.into(),
                _ => return,
            }
        }
        if self.call("profile.activate", params).await.is_some() {
            self.toast(format!("Profile {} active", slot + 1));
        }
        self.refresh_bar().await;
        self.refresh_all();
    }

    pub fn update_selection_label(&self) {
        let n = self.view.selected().len();
        self.selection_label.set_text(&match n {
            0 => "No keys selected".to_string(),
            1 => "1 key selected".to_string(),
            n => format!("{n} keys selected"),
        });
    }
}

fn set_swatch(swatch: &gtk::Box, color: &Value, slot: u64) {
    let css = gtk::CssProvider::new();
    css.load_from_string(&format!(
        ".profile-swatch-{slot} {{ background: rgb({},{},{}); border-radius: 50%; min-width: 18px; min-height: 18px; }}",
        color["r"].as_u64().unwrap_or(120),
        color["g"].as_u64().unwrap_or(120),
        color["b"].as_u64().unwrap_or(120)
    ));
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    for cls in swatch.css_classes() {
        if cls.starts_with("profile-swatch-") {
            swatch.remove_css_class(&cls);
        }
    }
    swatch.add_css_class(&format!("profile-swatch-{slot}"));
}

/// Sidebar entries: (page id, title, icon, section).
const NAV: &[(&str, &str, &str, &str)] = &[
    (
        "quick",
        "Quick Settings",
        "preferences-other-symbolic",
        "Profiles",
    ),
    ("profiles", "My Profiles", "view-grid-symbolic", "Profiles"),
    (
        "actuation",
        "Actuation Point",
        "object-flip-vertical-symbolic",
        "Keyboard Configuration",
    ),
    (
        "rapid",
        "Rapid Trigger",
        "media-playlist-repeat-symbolic",
        "Keyboard Configuration",
    ),
    (
        "rgb",
        "RGB Settings",
        "weather-clear-symbolic",
        "Keyboard Configuration",
    ),
    (
        "remap",
        "Remap",
        "input-keyboard-symbolic",
        "Keyboard Configuration",
    ),
    (
        "advanced",
        "Advanced Keys",
        "view-more-symbolic",
        "Keyboard Configuration",
    ),
    (
        "gamepad",
        "Gamepad",
        "input-gaming-symbolic",
        "Keyboard Configuration",
    ),
    ("settings", "Settings", "emblem-system-symbolic", "Settings"),
];

pub fn build(app: &adw::Application) -> adw::ApplicationWindow {
    let css = gtk::CssProvider::new();
    css.load_from_string(
        ".keytile { min-width: 44px; min-height: 36px; padding: 4px 6px; font-weight: 600; }
         .keygroup { font-weight: 700; margin-top: 10px; }
         .profile-name { font-weight: 700; font-size: 1.1em; }
         .sidebar-section { font-size: 0.8em; font-weight: 700; opacity: 0.6; margin: 10px 12px 4px 12px; }
         .card-title { font-weight: 700; font-size: 1.05em; }",
    );
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let backend = Backend::new();
    let view = KeyboardView::new();
    let toast = adw::ToastOverlay::new();

    // ----- profile bar
    let swatch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    swatch.set_valign(gtk::Align::Center);
    let name = gtk::Label::new(Some("…"));
    name.add_css_class("profile-name");
    let popover_list = gtk::ListBox::new();
    popover_list.add_css_class("boxed-list");
    popover_list.set_selection_mode(gtk::SelectionMode::None);
    let popover = gtk::Popover::new();
    let pbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let title = gtk::Label::new(Some("ONBOARD PROFILES"));
    title.add_css_class("sidebar-section");
    title.set_xalign(0.0);
    pbox.append(&title);
    pbox.append(&popover_list);
    pbox.set_size_request(320, -1);
    popover.set_child(Some(&pbox));
    let menu = gtk::MenuButton::builder()
        .popover(&popover)
        .icon_name("pan-down-symbolic")
        .build();
    menu.add_css_class("flat");
    let undo = gtk::Button::from_icon_name("edit-undo-symbolic");
    undo.set_tooltip_text(Some("Undo"));
    let redo = gtk::Button::from_icon_name("edit-redo-symbolic");
    redo.set_tooltip_text(Some("Redo"));
    let discard = gtk::Button::with_label("Discard");
    discard.set_tooltip_text(Some("Reload the profile from the keyboard"));
    let save = gtk::Button::with_label("Save to Keyboard");
    save.add_css_class("suggested-action");
    for b in [&undo, &redo, &discard, &save] {
        b.set_sensitive(false);
    }
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.set_margin_start(12);
    bar.set_margin_end(12);
    bar.set_margin_top(8);
    bar.append(&swatch);
    bar.append(&name);
    bar.append(&menu);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    bar.append(&spacer);
    bar.append(&undo);
    bar.append(&redo);
    bar.append(&discard);
    bar.append(&save);
    let profile_bar = ProfileBar {
        swatch,
        name,
        menu,
        undo,
        redo,
        save,
        discard,
        popover_list,
    };

    let selection_label = gtk::Label::new(Some("No keys selected"));
    selection_label.add_css_class("dim-label");
    let ctx = Ctx {
        backend,
        view: view.clone(),
        toast: toast.clone(),
        bar: profile_bar.clone(),
        refreshers: Rc::new(RefCell::new(HashMap::new())),
        current_page: Rc::new(RefCell::new("quick".into())),
        selection_label: selection_label.clone(),
    };

    // Profile bar actions.
    for (btn, method, msg) in [
        (&profile_bar.undo, "profile.undo", ""),
        (&profile_bar.redo, "profile.redo", ""),
        (&profile_bar.save, "profile.save", "Saved to keyboard"),
        (&profile_bar.discard, "profile.discard", "Changes discarded"),
    ] {
        let c = ctx.clone();
        btn.connect_clicked(move |_| {
            let c = c.clone();
            spawn(async move {
                if c.call(method, json!({})).await.is_some() && !msg.is_empty() {
                    c.toast(msg);
                }
                c.refresh_bar().await;
                c.refresh_page();
            });
        });
    }

    // ----- selection tools under the keyboard
    let sel_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    sel_row.set_margin_start(12);
    sel_row.set_margin_end(12);
    sel_row.append(&selection_label);
    let sp = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    sp.set_hexpand(true);
    sel_row.append(&sp);
    let select_all = gtk::Button::with_label("Select all keys");
    let discard_sel = gtk::Button::with_label("Discard selection");
    sel_row.append(&select_all);
    sel_row.append(&discard_sel);
    let v = view.clone();
    select_all.connect_clicked(move |_| v.select_all());
    let v = view.clone();
    discard_sel.connect_clicked(move |_| v.clear_selection());
    let c = ctx.clone();
    view.connect_selection_changed(move || c.update_selection_label());

    // ----- pages
    let stack = gtk::Stack::new();
    stack.set_vhomogeneous(false);
    stack.set_vexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    let page_overlays: HashMap<&str, Overlay> = [
        ("quick", Overlay::Values),
        ("profiles", Overlay::Lighting),
        ("actuation", Overlay::Values),
        ("rapid", Overlay::Values),
        ("rgb", Overlay::Lighting),
        ("remap", Overlay::Codes),
        ("advanced", Overlay::Badges),
        ("gamepad", Overlay::Badges),
        ("settings", Overlay::Lighting),
    ]
    .into_iter()
    .collect();
    stack.add_named(&scrolled(pages::quick::build(&ctx)), Some("quick"));
    stack.add_named(&scrolled(pages::profiles::build(&ctx)), Some("profiles"));
    stack.add_named(&scrolled(pages::actuation::build(&ctx)), Some("actuation"));
    stack.add_named(&scrolled(pages::rapid::build(&ctx)), Some("rapid"));
    stack.add_named(&scrolled(pages::rgb::build(&ctx)), Some("rgb"));
    stack.add_named(&scrolled(pages::remap::build(&ctx)), Some("remap"));
    stack.add_named(&scrolled(pages::advanced::build(&ctx)), Some("advanced"));
    stack.add_named(&scrolled(pages::gamepad::build(&ctx)), Some("gamepad"));
    stack.add_named(&scrolled(pages::settings::build(&ctx)), Some("settings"));

    // ----- sidebar
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.set_size_request(220, -1);
    let dev = gtk::Box::new(gtk::Orientation::Vertical, 2);
    dev.set_margin_top(12);
    dev.set_margin_start(12);
    dev.set_margin_bottom(6);
    let dev_title = gtk::Label::new(Some("My device"));
    dev_title.add_css_class("dim-label");
    dev_title.set_xalign(0.0);
    let dev_name = gtk::Label::new(Some("Connecting…"));
    dev_name.add_css_class("profile-name");
    dev_name.set_xalign(0.0);
    dev.append(&dev_title);
    dev.append(&dev_name);
    sidebar.append(&dev);
    let nav = gtk::ListBox::new();
    nav.add_css_class("navigation-sidebar");
    let mut last_section = "";
    for (id, title, icon, section) in NAV {
        if *section != last_section {
            let l = gtk::Label::new(Some(&section.to_uppercase()));
            l.add_css_class("sidebar-section");
            l.set_xalign(0.0);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&l));
            row.set_selectable(false);
            row.set_activatable(false);
            nav.append(&row);
            last_section = section;
        }
        let row = gtk::ListBoxRow::new();
        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        hb.set_margin_top(6);
        hb.set_margin_bottom(6);
        hb.set_margin_start(6);
        hb.append(&gtk::Image::from_icon_name(icon));
        hb.append(&gtk::Label::new(Some(title)));
        row.set_child(Some(&hb));
        row.set_widget_name(id);
        nav.append(&row);
    }
    let c = ctx.clone();
    let st = stack.clone();
    nav.connect_row_selected(move |_, row| {
        let Some(row) = row else { return };
        let id = row.widget_name().to_string();
        if id.is_empty() || st.child_by_name(&id).is_none() {
            return;
        }
        st.set_visible_child_name(&id);
        *c.current_page.borrow_mut() = id.clone();
        c.view
            .set_overlay(page_overlays.get(id.as_str()).copied().unwrap_or_default());
        c.refresh_page();
    });
    sidebar.append(&nav);
    let version = gtk::Label::new(Some(&format!("wooting {}", env!("CARGO_PKG_VERSION"))));
    version.add_css_class("dim-label");
    version.set_margin_top(12);
    version.set_vexpand(true);
    version.set_valign(gtk::Align::End);
    version.set_margin_bottom(12);
    sidebar.append(&version);

    // ----- content: profile bar, keyboard, selection row, page
    let kb_frame = gtk::Frame::new(None);
    kb_frame.set_child(Some(&view.area));
    kb_frame.set_margin_start(12);
    kb_frame.set_margin_end(12);
    kb_frame.set_margin_top(6);
    install_drop_target(&ctx);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&bar);
    content.append(&kb_frame);
    content.append(&sel_row);
    content.append(&stack);
    toast.set_child(Some(&content));

    let split = adw::OverlaySplitView::new();
    split.set_sidebar(Some(&sidebar));
    split.set_content(Some(&toast));
    split.set_sidebar_width_fraction(0.18);
    split.set_min_sidebar_width(200.0);

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&gtk::Label::new(Some("Wooting"))));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&split));

    let win = adw::ApplicationWindow::builder()
        .application(app)
        .title("Wooting")
        .default_width(1280)
        .default_height(860)
        .content(&toolbar)
        .build();

    // Initial load.
    let c = ctx.clone();
    let nav2 = nav.clone();
    spawn(async move {
        if let Some(v) = c.call("device.keys", json!({})).await {
            if let Some(l) = layout_from_json(&v) {
                c.view.set_layout(l);
            }
        }
        if let Some(info) = c.call("device.info", json!({})).await {
            dev_name.set_text(&format!(
                "{} · fw {}{}",
                info["model"].as_str().unwrap_or("?"),
                info["firmware"].as_str().unwrap_or("?"),
                if c.backend.is_daemon() {
                    ""
                } else {
                    " · direct"
                }
            ));
        }
        if let Some(v) = c.call("rgb.profile.get", json!({})).await {
            c.view.set_frame(frame_from_json(&v));
        }
        c.refresh_bar().await;
        let page = std::env::var("WOOTING_GTK_PAGE").unwrap_or_else(|_| "quick".into());
        let mut i = 0;
        while let Some(row) = nav2.row_at_index(i) {
            if row.widget_name() == page {
                nav2.select_row(Some(&row));
                break;
            }
            i += 1;
        }
        if nav2.selected_row().is_none() {
            if let Some(row) = nav2.row_at_index(1) {
                nav2.select_row(Some(&row));
            }
        }
    });
    win
}

fn install_drop_target(ctx: &Ctx) {
    let target = gtk::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    let c = ctx.clone();
    target.connect_drop(move |_, value, _, _| {
        let Ok(file) = value.get::<gio::File>() else {
            return false;
        };
        let Some(path) = file.path() else {
            return false;
        };
        let c = c.clone();
        spawn(async move {
            if let Some(v) = c.call("rgb.from_image", json!({"path": path})).await {
                c.view.set_frame(frame_from_json(&v));
                c.toast("Painted from image (live). Use RGB Settings → Store to keep it.");
            }
        });
        true
    });
    ctx.view.area.add_controller(target);
}

pub fn scrolled(page: gtk::Widget) -> gtk::ScrolledWindow {
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_propagate_natural_height(false);
    scroll.set_child(Some(&page));
    scroll
}

pub fn page_box() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 12);
    b.set_margin_start(12);
    b.set_margin_end(12);
    b.set_margin_top(6);
    b.set_margin_bottom(12);
    b
}

/// A titled card like Wootility's panels.
pub fn card(title: &str, description: Option<&str>) -> (gtk::Box, gtk::Box) {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 6);
    outer.add_css_class("card");
    outer.set_margin_top(4);
    let inner = gtk::Box::new(gtk::Orientation::Vertical, 8);
    inner.set_margin_top(12);
    inner.set_margin_bottom(12);
    inner.set_margin_start(14);
    inner.set_margin_end(14);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("card-title");
    t.set_xalign(0.0);
    inner.append(&t);
    if let Some(d) = description {
        let l = gtk::Label::new(Some(d));
        l.add_css_class("dim-label");
        l.set_xalign(0.0);
        l.set_wrap(true);
        inner.append(&l);
    }
    outer.append(&inner);
    (outer, inner)
}

pub fn mm_scale(min: f64, max: f64) -> gtk::Scale {
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, 0.05);
    s.set_hexpand(true);
    s.set_draw_value(true);
    s.set_digits(2);
    s.set_value_pos(gtk::PositionType::Right);
    s
}

/// Debounce slider changes so dragging does not spam the keyboard. Returns
/// the handler id so loaders can set values without triggering it (see
/// [`set_scale_quiet`]).
pub fn on_scale_settled(scale: &gtk::Scale, f: impl Fn(f64) + 'static) -> glib::SignalHandlerId {
    let f = Rc::new(f);
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    scale.connect_value_changed(move |s| {
        let value = s.value();
        if let Some(id) = pending.borrow_mut().take() {
            id.remove();
        }
        let p = pending.clone();
        let f = f.clone();
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
            p.borrow_mut().take();
            f(value);
        });
        *pending.borrow_mut() = Some(id);
    })
}

/// Set a slider from loaded data without firing its settled handler.
pub fn set_scale_quiet(scale: &gtk::Scale, handler: &glib::SignalHandlerId, value: f64) {
    scale.block_signal(handler);
    scale.set_value(value);
    scale.unblock_signal(handler);
}

/// Keycode names → labels for drop-downs and palettes (Wootility order).
pub const PALETTE: &[(&str, &[(&str, &str)])] = &[
    (
        "Basic characters",
        &[
            ("a", "A"),
            ("b", "B"),
            ("c", "C"),
            ("d", "D"),
            ("e", "E"),
            ("f", "F"),
            ("g", "G"),
            ("h", "H"),
            ("i", "I"),
            ("j", "J"),
            ("k", "K"),
            ("l", "L"),
            ("m", "M"),
            ("n", "N"),
            ("o", "O"),
            ("p", "P"),
            ("q", "Q"),
            ("r", "R"),
            ("s", "S"),
            ("t", "T"),
            ("u", "U"),
            ("v", "V"),
            ("w", "W"),
            ("x", "X"),
            ("y", "Y"),
            ("z", "Z"),
            ("1", "1"),
            ("2", "2"),
            ("3", "3"),
            ("4", "4"),
            ("5", "5"),
            ("6", "6"),
            ("7", "7"),
            ("8", "8"),
            ("9", "9"),
            ("0", "0"),
            ("minus", "- _"),
            ("equal", "= +"),
            ("lbracket", "[ {"),
            ("rbracket", "] }"),
            ("backslash", "\\ |"),
            ("semicolon", "; :"),
            ("quote", "' \""),
            ("grave", "` ~"),
            ("comma", ", <"),
            ("period", ". >"),
            ("slash", "/ ?"),
            ("nonus_backslash", "ISO \\"),
            ("nonus_hash", "ISO #"),
        ],
    ),
    (
        "Extended characters",
        &[
            ("esc", "Esc"),
            ("tab", "Tab"),
            ("capslock", "Caps"),
            ("backspace", "Backspace"),
            ("enter", "Enter"),
            ("space", "Space"),
            ("lctrl", "L-Ctrl"),
            ("rctrl", "R-Ctrl"),
            ("lshift", "L-Shift"),
            ("rshift", "R-Shift"),
            ("lalt", "L-Alt"),
            ("ralt", "R-Alt"),
            ("lgui", "L-Super"),
            ("rgui", "R-Super"),
            ("menu", "Menu"),
            ("ime_on", "IME On"),
            ("ime_off", "IME Off"),
            ("lang3", "Lang 3"),
            ("lang4", "Lang 4"),
            ("lang5", "Lang 5"),
            ("ro", "ろ/Ro"),
            ("kana", "Kana"),
            ("yen", "¥/Yen"),
            ("henkan", "Conv"),
            ("muhenkan", "Non-conv"),
            ("up", "↑"),
            ("down", "↓"),
            ("left", "←"),
            ("right", "→"),
            ("printscreen", "Prt Sc"),
            ("scrolllock", "Scr Lk"),
            ("pause", "Pause"),
            ("home", "Home"),
            ("end", "End"),
            ("insert", "Ins"),
            ("delete", "Del"),
            ("pageup", "Pg Up"),
            ("pagedown", "Pg Dn"),
            ("f1", "F1"),
            ("f2", "F2"),
            ("f3", "F3"),
            ("f4", "F4"),
            ("f5", "F5"),
            ("f6", "F6"),
            ("f7", "F7"),
            ("f8", "F8"),
            ("f9", "F9"),
            ("f10", "F10"),
            ("f11", "F11"),
            ("f12", "F12"),
            ("f13", "F13"),
            ("f14", "F14"),
            ("f15", "F15"),
            ("f16", "F16"),
            ("f17", "F17"),
            ("f18", "F18"),
            ("f19", "F19"),
            ("f20", "F20"),
            ("f21", "F21"),
            ("f22", "F22"),
            ("f23", "F23"),
            ("f24", "F24"),
            ("numlock", "Num Lk"),
            ("kp_1", "Num 1"),
            ("kp_2", "Num 2"),
            ("kp_3", "Num 3"),
            ("kp_4", "Num 4"),
            ("kp_5", "Num 5"),
            ("kp_6", "Num 6"),
            ("kp_7", "Num 7"),
            ("kp_8", "Num 8"),
            ("kp_9", "Num 9"),
            ("kp_0", "Num 0"),
            ("kp_slash", "Num /"),
            ("kp_asterisk", "Num *"),
            ("kp_minus", "Num -"),
            ("kp_plus", "Num +"),
            ("kp_enter", "Num ⏎"),
        ],
    ),
    (
        "Functions",
        &[
            ("media_player", "emblem-music-symbolic"),
            ("mail", "mail-unread-symbolic"),
            ("calculator", "accessories-calculator-symbolic"),
            ("file_explorer", "folder-symbolic"),
            ("search", "system-search-symbolic"),
            ("www", "web-browser-symbolic"),
            ("browser_back", "go-previous-symbolic"),
            ("browser_forward", "go-next-symbolic"),
            ("refresh", "view-refresh-symbolic"),
            ("bookmark", "user-bookmarks-symbolic"),
            ("power", "system-shutdown-symbolic"),
            ("sleep", "weather-clear-night-symbolic"),
            ("screen_switch", "video-display-symbolic"),
            ("app_grid", "view-app-grid-symbolic"),
            ("display_brightness_down", "Bright -"),
            ("display_brightness_up", "Bright +"),
            ("rgb_brightness_down", "RGB -"),
            ("rgb_brightness_up", "RGB +"),
            ("key_lock", "Key Lock"),
            ("fn1", "Fn 1"),
            ("fn2", "Fn 2"),
            ("fn3", "Fn 3"),
        ],
    ),
    (
        "Profiles",
        &[
            ("profile_1", "P1"),
            ("profile_2", "P2"),
            ("profile_3", "P3"),
            ("profile_4", "P4"),
            ("rgb_mode", "Mode"),
            ("profile_cycle", "Cycle"),
        ],
    ),
    (
        "Media and audio control",
        &[
            ("prev_track", "media-skip-backward-symbolic"),
            ("play_pause", "media-playback-start-symbolic"),
            ("stop", "media-playback-stop-symbolic"),
            ("next_track", "media-skip-forward-symbolic"),
            ("mute", "audio-volume-muted-symbolic"),
            ("volume_down", "audio-volume-low-symbolic"),
            ("volume_up", "audio-volume-high-symbolic"),
        ],
    ),
    (
        "Mouse",
        &[
            ("mouse_left", "Mouse L"),
            ("mouse_right", "Mouse R"),
            ("mouse_middle", "Mouse M"),
            ("mouse_back", "Mouse 4"),
            ("mouse_forward", "Mouse 5"),
        ],
    ),
    ("Other", &[("none", "Disable")]),
];

/// Build a searchable palette of key tiles; `on_pick(code_name)` runs on click.
pub fn key_palette(on_pick: Rc<dyn Fn(String)>) -> gtk::Box {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search for a key"));
    outer.append(&search);
    let mut tiles: Vec<(gtk::Button, String, String)> = Vec::new();
    let mut groups: Vec<(gtk::Label, gtk::FlowBox)> = Vec::new();
    for (title, keys) in PALETTE {
        let title_label = gtk::Label::new(Some(title));
        title_label.set_xalign(0.0);
        title_label.add_css_class("keygroup");
        outer.append(&title_label);
        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_max_children_per_line(24);
        flow.set_min_children_per_line(6);
        flow.set_column_spacing(4);
        flow.set_row_spacing(4);
        flow.set_homogeneous(true);
        flow.set_halign(gtk::Align::Start);
        for (name, label) in *keys {
            let btn = if label.ends_with("-symbolic") {
                gtk::Button::from_icon_name(label)
            } else {
                gtk::Button::with_label(label)
            };
            btn.add_css_class("keytile");
            btn.set_tooltip_text(Some(name));
            let on_pick = on_pick.clone();
            let n = name.to_string();
            btn.connect_clicked(move |_| on_pick(n.clone()));
            flow.insert(&btn, -1);
            tiles.push((btn, name.to_string(), label.to_string()));
        }
        outer.append(&flow);
        groups.push((title_label, flow));
    }
    let tiles = Rc::new(tiles);
    let groups = Rc::new(groups);
    search.connect_search_changed(move |e| {
        let q = e.text().to_lowercase();
        for (btn, name, label) in tiles.iter() {
            let hit = q.is_empty() || name.contains(&q) || label.to_lowercase().contains(&q);
            if let Some(p) = btn.parent() {
                p.set_visible(hit);
            }
        }
        for (title, flow) in groups.iter() {
            let any = (0..)
                .map_while(|i| flow.child_at_index(i))
                .any(|ch| ch.is_visible());
            title.set_visible(any);
        }
    });
    outer
}

/// Code-name choices for drop-downs: (name, label).
pub fn code_choices() -> Vec<(String, String)> {
    PALETTE
        .iter()
        .flat_map(|(_, keys)| keys.iter())
        .map(|(name, label)| {
            let label = if label.ends_with("-symbolic") {
                name.replace('_', " ")
            } else {
                label.to_string()
            };
            (name.to_string(), label)
        })
        .collect()
}

pub fn code_dropdown(choices: &[(String, String)]) -> gtk::DropDown {
    let labels: Vec<&str> = choices.iter().map(|(_, l)| l.as_str()).collect();
    let dd = gtk::DropDown::from_strings(&labels);
    dd.set_enable_search(true);
    dd.set_valign(gtk::Align::Center);
    dd
}

pub fn label_for(name: &str) -> String {
    wooting_proto::keycode::parse(name)
        .map(wooting_proto::keycode::label)
        .unwrap_or_else(|| name.to_string())
}

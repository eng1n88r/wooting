//! A drawn keyboard: shows per-key colours or labels depending on the
//! overlay mode, supports click/shift-click/drag selection and image drops.

use gtk::{cairo, gdk, gio, pango, prelude::*};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
use wooting_proto::{Frame, Layout, Rgb};

/// What the keycaps display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overlay {
    /// Physical labels, keycaps tinted with the lighting colours.
    #[default]
    Lighting,
    /// Mapping labels/icons (remap page).
    Codes,
    /// Per-key numeric values (actuation / rapid trigger pages).
    Values,
    /// Physical labels plus advanced-key badges.
    Badges,
}

#[derive(Default)]
pub struct State {
    pub layout: Option<Layout>,
    pub frame: Frame,
    pub selected: HashSet<String>,
    pub overlay: Overlay,
    /// `(code name, label)` by key id for [`Overlay::Codes`].
    pub codes: HashMap<String, (String, String)>,
    /// Text per key for [`Overlay::Values`] (e.g. "1.60").
    pub values: HashMap<String, String>,
    /// Keys highlighted in the Values overlay (e.g. rapid trigger on).
    pub marked: HashSet<String>,
    /// Keys with an advanced-key entry: id → short badge ("MT", "DKS").
    pub badges: HashMap<String, String>,
    /// Live analog travel 0..=255 by key id (drawn as a bar when non-zero).
    pub analog: HashMap<String, u8>,
    /// Whether the lighting colours are shown on the keycaps.
    pub show_colors: bool,
}

type IconCache = HashMap<(String, i32), Option<cairo::ImageSurface>>;

/// Codes drawn as Adwaita symbolic icons instead of text.
pub fn icon_for(code_name: &str) -> Option<&'static str> {
    Some(match code_name {
        "prev_track" => "media-skip-backward-symbolic",
        "play_pause" => "media-playback-start-symbolic",
        "next_track" => "media-skip-forward-symbolic",
        "stop" => "media-playback-stop-symbolic",
        "mute" => "audio-volume-muted-symbolic",
        "volume_down" => "audio-volume-low-symbolic",
        "volume_up" => "audio-volume-high-symbolic",
        "mail" => "mail-unread-symbolic",
        "calculator" => "accessories-calculator-symbolic",
        "file_explorer" => "folder-symbolic",
        "search" => "system-search-symbolic",
        "www" => "web-browser-symbolic",
        "browser_back" => "go-previous-symbolic",
        "browser_forward" => "go-next-symbolic",
        "refresh" => "view-refresh-symbolic",
        "bookmark" => "user-bookmarks-symbolic",
        "power" => "system-shutdown-symbolic",
        "sleep" => "weather-clear-night-symbolic",
        "media_player" => "emblem-music-symbolic",
        "display_brightness_down" => "display-brightness-symbolic",
        "display_brightness_up" => "display-brightness-symbolic",
        "mouse_left" | "mouse_right" | "mouse_middle" | "mouse_back" | "mouse_forward" => "input-mouse-symbolic",
        _ => return None,
    })
}

/// Load a symbolic icon as an alpha mask surface.
fn load_icon(name: &str, size: i32) -> Option<cairo::ImageSurface> {
    let display = gdk::Display::default()?;
    let theme = gtk::IconTheme::for_display(&display);
    let paintable = theme.lookup_icon(name, &[], size, 1, gtk::TextDirection::None, gtk::IconLookupFlags::empty());
    let file: gio::File = paintable.file()?;
    let pixbuf = gdk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &file.read(gio::Cancellable::NONE).ok()?,
        size,
        size,
        true,
        gio::Cancellable::NONE,
    )
    .ok()?;
    let w = pixbuf.width();
    let h = pixbuf.height();
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).ok()?;
    {
        let stride = surface.stride() as usize;
        let mut data = surface.data().ok()?;
        let src = pixbuf.read_pixel_bytes();
        let src_stride = pixbuf.rowstride() as usize;
        let n = pixbuf.n_channels() as usize;
        for y in 0..h as usize {
            for x in 0..w as usize {
                let a = if n == 4 { src[y * src_stride + x * n + 3] } else { 255 };
                let o = y * stride + x * 4;
                data[o] = a;
                data[o + 1] = a;
                data[o + 2] = a;
                data[o + 3] = a;
            }
        }
    }
    Some(surface)
}

#[derive(Clone)]
pub struct KeyboardView {
    pub area: gtk::DrawingArea,
    pub state: Rc<RefCell<State>>,
    icons: Rc<RefCell<IconCache>>,
    /// Called after the selection changes.
    on_select: Rc<RefCell<Option<Box<dyn Fn()>>>>,
}

const PAD: f64 = 8.0;

impl KeyboardView {
    pub fn new() -> KeyboardView {
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_content_height(300);
        let state = Rc::new(RefCell::new(State { show_colors: true, ..Default::default() }));
        let view = KeyboardView {
            area,
            state,
            icons: Rc::new(RefCell::new(HashMap::new())),
            on_select: Rc::new(RefCell::new(None)),
        };
        let v = view.clone();
        view.area.set_draw_func(move |_, cr, w, h| v.draw(cr, w as f64, h as f64));

        let click = gtk::GestureClick::new();
        let v = view.clone();
        click.connect_released(move |g, _, x, y| {
            let additive = g
                .current_event_state()
                .intersects(gdk::ModifierType::SHIFT_MASK | gdk::ModifierType::CONTROL_MASK);
            v.toggle_at(x, y, additive);
        });
        view.area.add_controller(click);

        // Drag to select several keys.
        let drag = gtk::GestureDrag::new();
        let v = view.clone();
        let start: Rc<RefCell<(f64, f64)>> = Rc::new(RefCell::new((0.0, 0.0)));
        let s0 = start.clone();
        drag.connect_drag_begin(move |_, x, y| *s0.borrow_mut() = (x, y));
        let s1 = start.clone();
        drag.connect_drag_end(move |g, dx, dy| {
            if dx.abs() < 6.0 && dy.abs() < 6.0 {
                return;
            }
            let (x0, y0) = *s1.borrow();
            let additive = g
                .current_event_state()
                .intersects(gdk::ModifierType::SHIFT_MASK | gdk::ModifierType::CONTROL_MASK);
            v.select_rect(x0, y0, x0 + dx, y0 + dy, additive);
        });
        view.area.add_controller(drag);
        view
    }

    pub fn connect_selection_changed(&self, f: impl Fn() + 'static) {
        *self.on_select.borrow_mut() = Some(Box::new(f));
    }

    fn selection_changed(&self) {
        self.area.queue_draw();
        if let Some(f) = self.on_select.borrow().as_ref() {
            f();
        }
    }

    pub fn set_layout(&self, layout: Layout) {
        self.state.borrow_mut().layout = Some(layout);
        self.area.queue_draw();
    }

    pub fn set_frame(&self, frame: Frame) {
        self.state.borrow_mut().frame = frame;
        self.area.queue_draw();
    }

    pub fn set_overlay(&self, overlay: Overlay) {
        self.state.borrow_mut().overlay = overlay;
        self.area.queue_draw();
    }

    pub fn selected(&self) -> Vec<String> {
        let mut v: Vec<String> = self.state.borrow().selected.iter().cloned().collect();
        v.sort();
        v
    }

    pub fn select_all(&self) {
        let mut st = self.state.borrow_mut();
        if let Some(l) = &st.layout {
            st.selected = l.keys.iter().map(|k| k.id.clone()).collect();
        }
        drop(st);
        self.selection_changed();
    }

    pub fn clear_selection(&self) {
        self.state.borrow_mut().selected.clear();
        self.selection_changed();
    }

    fn scale(&self, w: f64, h: f64) -> (f64, f64, f64) {
        let st = self.state.borrow();
        let Some(l) = &st.layout else { return (1.0, 0.0, 0.0) };
        let s = ((w - 2.0 * PAD) / l.width as f64).min((h - 2.0 * PAD) / l.height as f64);
        let ox = (w - s * l.width as f64) / 2.0;
        let oy = (h - s * l.height as f64) / 2.0;
        (s, ox, oy)
    }

    fn key_at(&self, x: f64, y: f64) -> Option<String> {
        let (w, h) = (self.area.width() as f64, self.area.height() as f64);
        let (s, ox, oy) = self.scale(w, h);
        let st = self.state.borrow();
        st.layout.as_ref().and_then(|l| {
            l.keys
                .iter()
                .find(|k| {
                    let kx = ox + k.x as f64 * s;
                    let ky = oy + k.y as f64 * s;
                    x >= kx && x < kx + k.w as f64 * s && y >= ky && y < ky + k.h as f64 * s
                })
                .map(|k| k.id.clone())
        })
    }

    fn toggle_at(&self, x: f64, y: f64, additive: bool) {
        let hit = self.key_at(x, y);
        {
            let mut st = self.state.borrow_mut();
            match hit {
                Some(id) => {
                    if !additive {
                        let only_this = st.selected.len() == 1 && st.selected.contains(&id);
                        st.selected.clear();
                        if only_this {
                            drop(st);
                            self.selection_changed();
                            return;
                        }
                    }
                    if !st.selected.remove(&id) {
                        st.selected.insert(id);
                    }
                }
                None => st.selected.clear(),
            }
        }
        self.selection_changed();
    }

    fn select_rect(&self, x0: f64, y0: f64, x1: f64, y1: f64, additive: bool) {
        let (w, h) = (self.area.width() as f64, self.area.height() as f64);
        let (s, ox, oy) = self.scale(w, h);
        let (lx, rx) = (x0.min(x1), x0.max(x1));
        let (ty, by) = (y0.min(y1), y0.max(y1));
        {
            let mut st = self.state.borrow_mut();
            if !additive {
                st.selected.clear();
            }
            let Some(l) = st.layout.clone() else { return };
            for k in &l.keys {
                let kx0 = ox + k.x as f64 * s;
                let ky0 = oy + k.y as f64 * s;
                let kx1 = kx0 + k.w as f64 * s;
                let ky1 = ky0 + k.h as f64 * s;
                if kx1 > lx && kx0 < rx && ky1 > ty && ky0 < by {
                    st.selected.insert(k.id.clone());
                }
            }
        }
        self.selection_changed();
    }

    fn draw(&self, cr: &cairo::Context, w: f64, h: f64) {
        let (s, ox, oy) = self.scale(w, h);
        let st = self.state.borrow();
        let Some(layout) = &st.layout else {
            cr.set_source_rgba(0.5, 0.5, 0.5, 1.0);
            cr.move_to(PAD, h / 2.0);
            let _ = cr.show_text("No keyboard");
            return;
        };
        let gap = s * 0.06;
        for key in &layout.keys {
            let cell = key.cells[0];
            let c = st.frame.get(cell[0] as usize, cell[1] as usize).unwrap_or(Rgb::BLACK);
            let x = ox + key.x as f64 * s + gap;
            let y = oy + key.y as f64 * s + gap;
            let kw = key.w as f64 * s - 2.0 * gap;
            let kh = key.h as f64 * s - 2.0 * gap;
            rounded_rect(cr, x, y, kw, kh, s * 0.12);
            // Keycap body: LED colour (lifted so black LEDs still read as keys)
            // or a neutral cap when colours are hidden.
            let (r, g, b) = if st.show_colors {
                let lift = |v: u8| 0.16 + (v as f64 / 255.0) * 0.84;
                (lift(c.r), lift(c.g), lift(c.b))
            } else {
                (0.20, 0.20, 0.22)
            };
            let marked = st.overlay == Overlay::Values && st.marked.contains(&key.id);
            if marked {
                cr.set_source_rgb(0.95, 0.75, 0.2);
            } else {
                cr.set_source_rgb(r, g, b);
            }
            let _ = cr.fill_preserve();
            if st.selected.contains(&key.id) {
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.set_line_width(2.5);
            } else {
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
                cr.set_line_width(1.0);
            }
            let _ = cr.stroke();
            // Analog bar along the bottom edge.
            if let Some(a) = st.analog.get(&key.id).copied().filter(|a| *a > 0) {
                let frac = a as f64 / 255.0;
                cr.set_source_rgba(1.0, 0.85, 0.2, 0.9);
                cr.rectangle(x + 2.0, y + kh - 4.0, (kw - 4.0) * frac, 3.0);
                let _ = cr.fill();
            }
            // Ink colour by luminance of the cap.
            let lum = if st.show_colors && !marked { 0.299 * c.r as f64 + 0.587 * c.g as f64 + 0.114 * c.b as f64 } else if marked { 200.0 } else { 0.0 };
            if lum > 150.0 {
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.85);
            } else {
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
            }
            if let Some(badge) = st.badges.get(&key.id) {
                if st.overlay != Overlay::Codes {
                    // Small badge in the top-right corner.
                    let layout_b = self.area.create_pango_layout(Some(badge));
                    let mut font = pango::FontDescription::new();
                    font.set_absolute_size((s * 0.16).clamp(6.0, 10.0) * pango::SCALE as f64);
                    font.set_weight(pango::Weight::Bold);
                    layout_b.set_font_description(Some(&font));
                    let (_, lg) = layout_b.pixel_extents();
                    cr.move_to(x + kw - lg.width() as f64 - 3.0, y + 2.0);
                    pangocairo::functions::show_layout(cr, &layout_b);
                }
            }
            let (code_name, text) = match st.overlay {
                Overlay::Codes => st.codes.get(&key.id).cloned().unwrap_or_else(|| (String::new(), "·".into())),
                Overlay::Values => (String::new(), st.values.get(&key.id).cloned().unwrap_or_default()),
                Overlay::Lighting | Overlay::Badges => (String::new(), key.label.clone()),
            };
            if let Some(icon) = icon_for(&code_name) {
                let size = ((kw.min(kh)) * 0.55).round().max(8.0) as i32;
                let surface = self
                    .icons
                    .borrow_mut()
                    .entry((icon.to_string(), size))
                    .or_insert_with(|| load_icon(icon, size))
                    .clone();
                if let Some(surf) = surface {
                    let ix = x + (kw - surf.width() as f64) / 2.0;
                    let iy = y + (kh - surf.height() as f64) / 2.0;
                    let _ = cr.mask_surface(&surf, ix, iy);
                    continue;
                }
            }
            if text.is_empty() {
                continue;
            }
            // Values overlay shows the physical label small on top and the value below.
            if st.overlay == Overlay::Values {
                let small = self.area.create_pango_layout(Some(&key.label));
                let mut font = pango::FontDescription::new();
                font.set_absolute_size((s * 0.16).clamp(6.0, 10.0) * pango::SCALE as f64);
                small.set_font_description(Some(&font));
                small.set_width(((kw - 6.0) * pango::SCALE as f64) as i32);
                small.set_ellipsize(pango::EllipsizeMode::End);
                cr.move_to(x + 3.0, y + 2.0);
                pangocairo::functions::show_layout(cr, &small);
            }
            let layout_t = self.area.create_pango_layout(Some(&text));
            layout_t.set_width(((kw - 6.0) * pango::SCALE as f64) as i32);
            layout_t.set_ellipsize(pango::EllipsizeMode::End);
            layout_t.set_alignment(pango::Alignment::Center);
            let mut size = (s * 0.28).clamp(8.0, 16.0);
            loop {
                let mut font = pango::FontDescription::new();
                font.set_absolute_size(size * pango::SCALE as f64);
                layout_t.set_font_description(Some(&font));
                let (ink, _) = layout_t.pixel_extents();
                let fits = !layout_t.is_ellipsized() && ink.width() as f64 <= kw - 6.0;
                if fits || size <= 7.0 {
                    break;
                }
                size -= 1.0;
            }
            let (_, logical) = layout_t.pixel_extents();
            let ty = if st.overlay == Overlay::Values {
                y + kh - logical.height() as f64 - 3.0
            } else {
                y + (kh - logical.height() as f64) / 2.0
            };
            cr.move_to(x + 3.0, ty);
            pangocairo::functions::show_layout(cr, &layout_t);
        }
    }
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

pub fn frame_from_json(v: &serde_json::Value) -> Frame {
    let mut f = Frame::default();
    for (r, row) in v["rows"].as_array().into_iter().flatten().enumerate() {
        for (c, cell) in row.as_array().into_iter().flatten().enumerate() {
            if let Some(rgb) = cell.as_str().and_then(Rgb::parse_hex) {
                f.set(r, c, rgb);
            }
        }
    }
    f
}

pub fn rgb_to_gdk(c: Rgb) -> gdk::RGBA {
    gdk::RGBA::new(c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0)
}

pub fn gdk_to_rgb(c: &gdk::RGBA) -> Rgb {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb::new(q(c.red()), q(c.green()), q(c.blue()))
}

pub fn layout_from_json(v: &serde_json::Value) -> Option<Layout> {
    let keys: Vec<wooting_proto::Key> = serde_json::from_value(v["keys"].clone()).ok()?;
    let width = keys.iter().map(|k| k.x + k.w).fold(0.0, f32::max);
    let height = keys.iter().map(|k| k.y + k.h).fold(0.0, f32::max);
    Some(Layout {
        name: v["layout"].as_str().unwrap_or("Wooting").to_string(),
        cols: 21,
        width,
        height,
        keys,
    })
}

impl Default for KeyboardView {
    fn default() -> Self {
        Self::new()
    }
}

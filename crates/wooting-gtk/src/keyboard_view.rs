//! A drawn keyboard: shows per-key colours, supports click/shift-click
//! selection and drag-and-drop of image files.

use gtk::{cairo, gdk, gio, pango, prelude::*};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
use wooting_proto::{Frame, Layout, Rgb};

#[derive(Default)]
pub struct State {
    pub layout: Option<Layout>,
    pub frame: Frame,
    pub selected: HashSet<String>,
    /// `(code name, label)` from the mapping profile, by key id (keymap page).
    pub codes: HashMap<String, (String, String)>,
    pub show_codes: bool,
}

type IconCache = HashMap<(String, i32), Option<cairo::ImageSurface>>;

/// Codes drawn as Adwaita symbolic icons instead of text.
fn icon_for(code_name: &str) -> Option<&'static str> {
    Some(match code_name {
        "prev_track" => "media-skip-backward-symbolic",
        "play_pause" => "media-playback-start-symbolic",
        "next_track" => "media-skip-forward-symbolic",
        "mute" => "audio-volume-muted-symbolic",
        "volume_down" => "audio-volume-low-symbolic",
        "volume_up" => "audio-volume-high-symbolic",
        _ => return None,
    })
}

/// Load a symbolic icon as an alpha mask surface.
fn load_icon(name: &str, size: i32) -> Option<cairo::ImageSurface> {
    let display = gdk::Display::default()?;
    let theme = gtk::IconTheme::for_display(&display);
    let paintable = theme.lookup_icon(name, &[], size, 1, gtk::TextDirection::None, gtk::IconLookupFlags::empty());
    let file: gio::File = paintable.file()?;
    let texture = gdk::Texture::from_file(&file).ok()?;
    // Rasterise at the requested size: load the SVG scaled through a pixbuf.
    let pixbuf = gdk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &file.read(gio::Cancellable::NONE).ok()?,
        size,
        size,
        true,
        gio::Cancellable::NONE,
    )
    .ok()?;
    let _ = texture;
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
                // Premultiplied white with the icon's alpha; the draw step masks with the text colour.
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
    /// Rendered symbolic icons by (icon name, pixel size).
    icons: Rc<RefCell<IconCache>>,
}

const PAD: f64 = 8.0;

impl KeyboardView {
    pub fn new() -> KeyboardView {
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_content_height(300);
        let state = Rc::new(RefCell::new(State::default()));
        let view = KeyboardView { area, state, icons: Rc::new(RefCell::new(HashMap::new())) };
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
        view
    }

    pub fn set_layout(&self, layout: Layout) {
        self.state.borrow_mut().layout = Some(layout);
        self.area.queue_draw();
    }

    pub fn set_frame(&self, frame: Frame) {
        self.state.borrow_mut().frame = frame;
        self.area.queue_draw();
    }

    pub fn selected(&self) -> Vec<String> {
        let mut v: Vec<String> = self.state.borrow().selected.iter().cloned().collect();
        v.sort();
        v
    }

    fn scale(&self, w: f64, h: f64) -> (f64, f64, f64) {
        let st = self.state.borrow();
        let Some(l) = &st.layout else { return (1.0, 0.0, 0.0) };
        let s = ((w - 2.0 * PAD) / l.width as f64).min((h - 2.0 * PAD) / l.height as f64);
        let ox = (w - s * l.width as f64) / 2.0;
        let oy = (h - s * l.height as f64) / 2.0;
        (s, ox, oy)
    }

    fn toggle_at(&self, x: f64, y: f64, additive: bool) {
        let (w, h) = (self.area.width() as f64, self.area.height() as f64);
        let (s, ox, oy) = self.scale(w, h);
        let hit = {
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
        };
        let mut st = self.state.borrow_mut();
        match hit {
            Some(id) => {
                if !additive {
                    let only_this = st.selected.len() == 1 && st.selected.contains(&id);
                    st.selected.clear();
                    if only_this {
                        drop(st);
                        self.area.queue_draw();
                        return;
                    }
                }
                if !st.selected.remove(&id) {
                    st.selected.insert(id);
                }
            }
            None => st.selected.clear(),
        }
        drop(st);
        self.area.queue_draw();
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
            // Keycap body: LED colour, lifted so black LEDs still read as keys.
            let lift = |v: u8| 0.16 + (v as f64 / 255.0) * 0.84;
            cr.set_source_rgb(lift(c.r), lift(c.g), lift(c.b));
            let _ = cr.fill_preserve();
            if st.selected.contains(&key.id) {
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.set_line_width(2.5);
            } else {
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.35);
                cr.set_line_width(1.0);
            }
            let _ = cr.stroke();
            // Label.
            let lum = 0.299 * c.r as f64 + 0.587 * c.g as f64 + 0.114 * c.b as f64;
            if lum > 150.0 {
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.85);
            } else {
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
            }
            let (code_name, text) = if st.show_codes {
                st.codes
                    .get(&key.id)
                    .cloned()
                    .unwrap_or_else(|| (String::new(), "·".into()))
            } else {
                (String::new(), key.label.clone())
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
            // Pango (not cairo's toy text API) so symbol glyphs fall back to
            // whatever font has them; shrink to fit, then ellipsise.
            let layout = self.area.create_pango_layout(Some(&text));
            layout.set_width(((kw - 6.0) * pango::SCALE as f64) as i32);
            layout.set_ellipsize(pango::EllipsizeMode::End);
            layout.set_alignment(pango::Alignment::Center);
            let mut size = (s * 0.28).clamp(8.0, 16.0);
            loop {
                let mut font = pango::FontDescription::new();
                font.set_absolute_size(size * pango::SCALE as f64);
                layout.set_font_description(Some(&font));
                let (ink, _) = layout.pixel_extents();
                let fits = layout.is_ellipsized() == false && ink.width() as f64 <= kw - 6.0;
                if fits || size <= 7.0 {
                    break;
                }
                size -= 1.0;
            }
            let (_, logical) = layout.pixel_extents();
            cr.move_to(x + 3.0, y + (kh - logical.height() as f64) / 2.0);
            pangocairo::functions::show_layout(cr, &layout);
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
    // device.keys returns the same shape as the TOML layout, minus the sizes.
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

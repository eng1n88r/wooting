//! A drawn keyboard: shows per-key colours, supports click/shift-click
//! selection and drag-and-drop of image files.

use gtk::{cairo, gdk, prelude::*};
use std::{cell::RefCell, collections::HashSet, rc::Rc};
use wooting_proto::{Frame, Layout, Rgb};

#[derive(Default)]
pub struct State {
    pub layout: Option<Layout>,
    pub frame: Frame,
    pub selected: HashSet<String>,
    /// Key code names from the mapping profile, by key id (keymap page).
    pub codes: std::collections::HashMap<String, String>,
    pub show_codes: bool,
}

#[derive(Clone)]
pub struct KeyboardView {
    pub area: gtk::DrawingArea,
    pub state: Rc<RefCell<State>>,
}

const PAD: f64 = 8.0;

impl KeyboardView {
    pub fn new() -> KeyboardView {
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_content_height(300);
        let state = Rc::new(RefCell::new(State::default()));
        let view = KeyboardView { area, state };
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
            let text = if st.show_codes {
                st.codes
                    .get(&key.id)
                    .cloned()
                    .unwrap_or_else(|| "·".into())
            } else {
                key.label.clone()
            };
            cr.set_font_size((s * 0.28).clamp(8.0, 16.0));
            if let Ok(ext) = cr.text_extents(&text) {
                let tx = x + (kw - ext.width()) / 2.0 - ext.x_bearing();
                let ty = y + (kh + ext.height()) / 2.0;
                cr.move_to(tx, ty);
                let _ = cr.show_text(&text);
            }
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

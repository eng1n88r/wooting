//! Turn an image into a per-key colour frame.

use crate::Result;
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use serde::{Deserialize, Serialize};
use wooting_proto::{Frame, Layout, Rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    /// Scale so the image covers the whole keyboard, cropping the overflow.
    #[default]
    Cover,
    /// Scale so the whole image is visible; keys outside it stay black.
    Fit,
    /// Stretch to the keyboard's aspect ratio.
    Stretch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageOptions {
    #[serde(default)]
    pub mode: FitMode,
    /// Gamma applied to the sampled colours (1.0 = none). Values below 1
    /// brighten mid-tones, which usually looks better on LEDs.
    #[serde(default = "default_gamma")]
    pub gamma: f32,
    /// Overall brightness multiplier 0..=1.
    #[serde(default = "one")]
    pub brightness: f32,
}

fn default_gamma() -> f32 {
    0.8
}
fn one() -> f32 {
    1.0
}

impl Default for ImageOptions {
    fn default() -> Self {
        ImageOptions {
            mode: FitMode::Cover,
            gamma: default_gamma(),
            brightness: 1.0,
        }
    }
}

/// Pixels per key unit used for sampling.
const PX_PER_UNIT: f32 = 24.0;

pub fn frame_from_image(img: &DynamicImage, layout: &Layout, opts: &ImageOptions) -> Frame {
    let target_w = (layout.width * PX_PER_UNIT).round().max(1.0) as u32;
    let target_h = (layout.height * PX_PER_UNIT).round().max(1.0) as u32;
    let (iw, ih) = img.dimensions();
    let (sx, sy) = (target_w as f32 / iw as f32, target_h as f32 / ih as f32);
    let scale = match opts.mode {
        FitMode::Cover => sx.max(sy),
        FitMode::Fit => sx.min(sy),
        FitMode::Stretch => 0.0,
    };
    let (rw, rh) = if scale > 0.0 {
        (
            ((iw as f32 * scale).round() as u32).max(1),
            ((ih as f32 * scale).round() as u32).max(1),
        )
    } else {
        (target_w, target_h)
    };
    let resized = img.resize_exact(rw, rh, FilterType::Triangle).to_rgb8();
    // Offset of the resized image inside the keyboard canvas (centred).
    let ox = (target_w as f32 - rw as f32) / 2.0;
    let oy = (target_h as f32 - rh as f32) / 2.0;

    let mut frame = Frame::default();
    for key in &layout.keys {
        for (i, cell) in key.cells.iter().enumerate() {
            let (cx, cy) = key.cell_center(i);
            let half = PX_PER_UNIT * 0.35;
            let px = cx * PX_PER_UNIT - ox;
            let py = cy * PX_PER_UNIT - oy;
            let colour = average(&resized, px, py, half, opts);
            frame.set(cell[0] as usize, cell[1] as usize, colour);
        }
    }
    frame
}

fn average(img: &image::RgbImage, cx: f32, cy: f32, half: f32, opts: &ImageOptions) -> Rgb {
    let (w, h) = img.dimensions();
    let x0 = (cx - half).floor().max(0.0) as u32;
    let y0 = (cy - half).floor().max(0.0) as u32;
    let x1 = ((cx + half).ceil() as i64).clamp(0, w as i64) as u32;
    let y1 = ((cy + half).ceil() as i64).clamp(0, h as i64) as u32;
    if x1 <= x0 || y1 <= y0 {
        return Rgb::BLACK;
    }
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = img.get_pixel(x, y);
            r += p[0] as u64;
            g += p[1] as u64;
            b += p[2] as u64;
            n += 1;
        }
    }
    let adj = |v: u64| -> u8 {
        let lin = (v as f32 / n as f32) / 255.0;
        let out = lin.powf(opts.gamma) * opts.brightness.clamp(0.0, 1.0);
        (out * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Rgb::new(adj(r), adj(g), adj(b))
}

pub fn load_image(path: &std::path::Path) -> Result<DynamicImage> {
    // Guess from content, not extension: Omarchy's `background` symlink has none.
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    Ok(reader.decode()?)
}

pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage> {
    Ok(image::load_from_memory(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_image_gives_solid_frame() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(64, 32, image::Rgb([200, 100, 50])));
        let layout = Layout::sixty_he();
        let opts = ImageOptions {
            gamma: 1.0,
            ..Default::default()
        };
        let f = frame_from_image(&img, &layout, &opts);
        let esc = layout.key("esc").unwrap().cells[0];
        assert_eq!(f.get(esc[0] as usize, esc[1] as usize), Some(Rgb::new(200, 100, 50)));
        let space = layout.key("space").unwrap().cells[2];
        assert_eq!(f.get(space[0] as usize, space[1] as usize), Some(Rgb::new(200, 100, 50)));
    }

    #[test]
    fn left_right_gradient_maps_across_keys() {
        let mut img = image::RgbImage::new(150, 50);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([if x < 75 { 255 } else { 0 }, 0, if x < 75 { 0 } else { 255 }]);
        }
        let layout = Layout::sixty_he();
        let opts = ImageOptions {
            gamma: 1.0,
            mode: FitMode::Stretch,
            ..Default::default()
        };
        let f = frame_from_image(&DynamicImage::ImageRgb8(img), &layout, &opts);
        let esc = layout.key("esc").unwrap().cells[0];
        let bs = layout.key("backspace").unwrap().cells[0];
        assert!(f.get(esc[0] as usize, esc[1] as usize).unwrap().r > 200);
        assert!(f.get(bs[0] as usize, bs[1] as usize).unwrap().b > 200);
    }
}

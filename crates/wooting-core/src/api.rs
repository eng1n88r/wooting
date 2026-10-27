//! The one operation set shared by the daemon's HTTP and socket servers,
//! the MCP server and the CLI. Every surface ends up in [`Api::call`].

use crate::{
    backup, image_map,
    image_map::ImageOptions,
    theme, Error, Keyboard, Result,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Mutex};
use wooting_proto::{keycode, pb, Command, Frame, Rgb, COLS, ROWS};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: i32,
    pub message: String,
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        let code = match &e {
            Error::UnknownKey(_) | Error::NoSuchProfile(_) => -32602,
            Error::Hid(wooting_hid::Error::NotFound) => -32001,
            _ => -32000,
        };
        ApiError {
            code,
            message: e.to_string(),
        }
    }
}

impl ApiError {
    pub fn invalid(msg: impl Into<String>) -> ApiError {
        ApiError {
            code: -32602,
            message: msg.into(),
        }
    }
    pub fn method_not_found(name: &str) -> ApiError {
        ApiError {
            code: -32601,
            message: format!("unknown method {name:?}"),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for ApiError {}

pub type ApiResult = std::result::Result<Value, ApiError>;

/// Description of one method, used to build MCP tools and CLI help.
#[derive(Debug, Clone, Serialize)]
pub struct MethodInfo {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON schema of the params object.
    pub params: Value,
}

pub fn methods() -> Vec<MethodInfo> {
    let color = json!({"type": "string", "description": "Colour as #rrggbb"});
    let obj = |props: Value, required: &[&str]| {
        json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
    };
    vec![
        MethodInfo { name: "device.info", description: "Model, serial, firmware, active profile and whether direct RGB mode is on.", params: obj(json!({}), &[]) },
        MethodInfo { name: "device.keys", description: "List key ids that can be used with rgb.set_key / rgb.set_keys, with labels and positions.", params: obj(json!({}), &[]) },
        MethodInfo { name: "profile.list", description: "List keyboard profiles (slot, name, colour, active).", params: obj(json!({}), &[]) },
        MethodInfo { name: "profile.activate", description: "Switch the keyboard to profile slot N (0-based).", params: obj(json!({"slot": {"type": "integer", "minimum": 0}}), &["slot"]) },
        MethodInfo { name: "rgb.fill", description: "Light every key with one colour (volatile direct mode).", params: obj(json!({"color": color}), &["color"]) },
        MethodInfo { name: "rgb.set_key", description: "Set one key's colour by id (volatile direct mode).", params: obj(json!({"key": {"type": "string"}, "color": color}), &["key", "color"]) },
        MethodInfo { name: "rgb.set_keys", description: "Set many keys at once: {\"colors\": {\"esc\": \"#ff0000\", \"space\": \"#00ff00\"}}.", params: obj(json!({"colors": {"type": "object", "additionalProperties": color}}), &["colors"]) },
        MethodInfo { name: "rgb.set_frame", description: "Set the whole LED matrix: rows is 6 rows of up to 21 colours (#rrggbb or null).", params: obj(json!({"rows": {"type": "array", "items": {"type": "array", "items": {"type": ["string", "null"]}}}}), &["rows"]) },
        MethodInfo { name: "rgb.get_frame", description: "Current direct-mode frame as 6 rows of #rrggbb.", params: obj(json!({}), &[]) },
        MethodInfo { name: "rgb.from_image", description: "Paint the keyboard with an image (file path or base64 data). mode: cover|fit|stretch, gamma default 0.8.", params: obj(json!({"path": {"type": "string"}, "data_base64": {"type": "string"}, "mode": {"type": "string", "enum": ["cover", "fit", "stretch"]}, "gamma": {"type": "number"}, "brightness": {"type": "number"}}), &[]) },
        MethodInfo { name: "rgb.release", description: "Leave direct mode and return control of the LEDs to the keyboard's own lighting.", params: obj(json!({}), &[]) },
        MethodInfo { name: "rgb.brightness.get", description: "Brightness (0-255) stored in an RGB profile slot.", params: obj(json!({"slot": {"type": "integer", "minimum": 0, "description": "RGB profile slot; default = active profile"}}), &[]) },
        MethodInfo { name: "rgb.brightness.set", description: "Set stored RGB profile brightness (0-255). Writes flash.", params: obj(json!({"value": {"type": "integer", "minimum": 0, "maximum": 255}, "slot": {"type": "integer", "minimum": 0}}), &["value"]) },
        MethodInfo { name: "rgb.profile.get", description: "Colours stored in an RGB profile slot (default: active profile).", params: obj(json!({"slot": {"type": "integer", "minimum": 0, "description": "RGB profile slot; default = active profile"}}), &[]) },
        MethodInfo { name: "rgb.profile.set", description: "Write colours (6 rows of #rrggbb) and optionally brightness straight into an RGB profile slot (default: active). Writes flash.", params: obj(json!({"rows": {"type": "array", "items": {"type": "array", "items": {"type": ["string", "null"]}}}, "brightness": {"type": "integer"}, "slot": {"type": "integer"}}), &["rows"]) },
        MethodInfo { name: "rgb.save", description: "Persist the current direct-mode frame into an RGB profile slot (default: active profile) so it survives unplugging. Writes flash.", params: obj(json!({"slot": {"type": "integer", "minimum": 0, "description": "RGB profile slot; default = active profile"}}), &[]) },
        MethodInfo { name: "theme.apply", description: "Colour the keyboard from an Omarchy theme directory (default: the current theme). persist=true also saves it to the keyboard.", params: obj(json!({"dir": {"type": "string"}, "persist": {"type": "boolean"}}), &[]) },
        MethodInfo { name: "settings.get", description: "Actuation point, rapid trigger and Tachyon settings stored in a profile slot (default: active).", params: obj(json!({"slot": {"type": "integer"}}), &[]) },
        MethodInfo { name: "settings.set", description: "Change actuation (mm, 0.1-4.0), rapid trigger (on/off, press/release sensitivity in mm) and Tachyon for all keys. Applies live; persist=true also writes the active profile to flash.", params: obj(json!({"actuation_mm": {"type": "number"}, "rapid_trigger": {"type": "boolean"}, "rt_press_mm": {"type": "number"}, "rt_release_mm": {"type": ["number", "null"], "description": "null = same as press"}, "tachyon": {"type": "boolean"}, "persist": {"type": "boolean"}}), &[]) },
        MethodInfo { name: "actuation.keys.get", description: "Per-key actuation overrides of a profile slot (default: active) as {key_id: mm}.", params: obj(json!({"slot": {"type": "integer"}}), &[]) },
        MethodInfo { name: "actuation.keys.set", description: "Set per-key actuation overrides: {\"keys\": {\"a\": 2.5, \"s\": null}} (null removes the override). persist=true writes flash.", params: obj(json!({"keys": {"type": "object", "additionalProperties": {"type": ["number", "null"]}}, "persist": {"type": "boolean"}}), &["keys"]) },
        MethodInfo { name: "rapid_trigger.keys.get", description: "Keys with rapid trigger enabled individually in a profile slot (default: active).", params: obj(json!({"slot": {"type": "integer"}}), &[]) },
        MethodInfo { name: "rapid_trigger.keys.set", description: "Enable/disable rapid trigger per key: {\"keys\": {\"w\": true, \"a\": false}}. persist=true writes flash.", params: obj(json!({"keys": {"type": "object", "additionalProperties": {"type": "boolean"}}, "persist": {"type": "boolean"}}), &["keys"]) },
        MethodInfo { name: "rgb.effects.get", description: "Effect layers and brightness of an RGB profile slot (default: active). Layer kinds: wave, scan, breath, cycle, ripple, trail, touch, jelly, aoe, mixing, rt_display, noise, spiral; empty = static.", params: obj(json!({"slot": {"type": "integer"}}), &[]) },
        MethodInfo { name: "rgb.effects.set", description: "Replace the effect layers, e.g. {\"layers\": [{\"wave\": {\"look\": 0, \"speed\": 16, \"direction\": 0}}]} ([] = static). persist=true writes flash.", params: obj(json!({"layers": {"type": "array"}, "brightness": {"type": "integer"}, "persist": {"type": "boolean"}}), &["layers"]) },
        MethodInfo { name: "settings.device.get", description: "Device-wide hardware flags: compatibility mode (6KRO) and gamepad mode.", params: obj(json!({}), &[]) },
        MethodInfo { name: "settings.device.set", description: "Set compatibility mode (6KRO) or gamepad mode. The keyboard restarts; gamepad mode changes its USB identity.", params: obj(json!({"compat_mode": {"type": "boolean"}, "gamepad_mode": {"type": "boolean"}}), &[]) },
        MethodInfo { name: "settings.global.get", description: "Global settings (RGB power saving timers).", params: obj(json!({}), &[]) },
        MethodInfo { name: "settings.global.set", description: "Device-wide settings: RGB power saving minutes (0 = never), inverted Num Lock indicator, debounce filter.", params: obj(json!({"dim_after_minutes": {"type": "integer"}, "off_after_minutes": {"type": "integer"}, "numlock_inverted": {"type": "boolean"}, "debounce_filter": {"type": "boolean"}}), &[]) },
        MethodInfo { name: "keymap.get", description: "Key mapping of a layer (0 = main, 1/2 = Fn layers) as {key_id: code_name} plus the raw matrix. slot defaults to the active profile.", params: obj(json!({"layer": {"type": "integer", "minimum": 0, "maximum": 3}, "slot": {"type": "integer"}}), &[]) },
        MethodInfo { name: "keymap.set_key", description: "Remap one key: code is a key name from keycodes.list (e.g. \"esc\", \"f5\", \"lctrl\", \"none\"). Applies live to the active profile; persist=true also writes flash.", params: obj(json!({"key": {"type": "string"}, "code": {"type": "string"}, "layer": {"type": "integer", "minimum": 0, "maximum": 3}, "persist": {"type": "boolean"}}), &["key", "code"]) },
        MethodInfo { name: "keymap.set", description: "Remap several keys at once: {\"keys\": {\"capslock\": \"esc\", \"esc\": \"grave\"}}.", params: obj(json!({"keys": {"type": "object", "additionalProperties": {"type": "string"}}, "layer": {"type": "integer"}, "persist": {"type": "boolean"}}), &["keys"]) },
        MethodInfo { name: "keymap.reset", description: "Restore the factory mapping of a layer.", params: obj(json!({"layer": {"type": "integer"}, "persist": {"type": "boolean"}}), &[]) },
        MethodInfo { name: "keycodes.list", description: "All key code names accepted by keymap.set_key.", params: obj(json!({}), &[]) },
        MethodInfo { name: "advanced_keys.get", description: "Advanced keys (mod-tap) of a profile slot (default: active): key id, hold action, tap action.", params: obj(json!({"slot": {"type": "integer"}}), &[]) },
        MethodInfo { name: "advanced_keys.set_mod_tap", description: "Make a key a mod-tap: 'hold' is sent while held, 'tap' when tapped (key code names). Written to the active profile.", params: obj(json!({"key": {"type": "string"}, "hold": {"type": "string"}, "tap": {"type": "string"}}), &["key", "hold", "tap"]) },
        MethodInfo { name: "advanced_keys.set", description: "Add or replace an advanced key. kind=mod_tap {tap,hold} | toggle {code} | rappy_snappy {other_key} | socd {other_key, mode} | dks {codes:[4 names], masks:[4 ints]?}. Written to the active profile.", params: obj(json!({"key": {"type": "string"}, "kind": {"type": "string", "enum": ["mod_tap", "toggle", "rappy_snappy", "socd", "dks"]}, "tap": {"type": "string"}, "hold": {"type": "string"}, "code": {"type": "string"}, "other_key": {"type": "string"}, "mode": {"type": "integer"}, "codes": {"type": "array", "items": {"type": "string"}}, "masks": {"type": "array", "items": {"type": "integer"}}}), &["key", "kind"]) },
        MethodInfo { name: "advanced_keys.remove", description: "Remove the advanced-key entry from a key.", params: obj(json!({"key": {"type": "string"}}), &["key"]) },
        MethodInfo { name: "profile.rename", description: "Rename a profile slot and/or set its badge colour.", params: obj(json!({"slot": {"type": "integer"}, "name": {"type": "string"}, "color": color}), &["slot"]) },
        MethodInfo { name: "profile.delete", description: "Delete an onboard profile slot. Irreversible; needs confirm=true.", params: obj(json!({"slot": {"type": "integer"}, "confirm": {"type": "boolean"}}), &["slot"]) },
        MethodInfo { name: "backup.create", description: "Dump every readable profile/setting to a JSON file and return its path.", params: obj(json!({}), &[]) },
        MethodInfo { name: "raw.command", description: "Send a raw feature command by name or id with up to 4 byte params. Dangerous commands need confirm=true.", params: obj(json!({"command": {"type": "string"}, "params": {"type": "array", "items": {"type": "integer"}, "maxItems": 4}, "confirm": {"type": "boolean"}}), &["command"]) },
    ]
}

/// Owns the keyboard and serialises access to it.
pub struct Api {
    selector: Option<String>,
    kb: Mutex<Option<Keyboard>>,
}

impl Api {
    pub fn new(selector: Option<String>) -> Api {
        Api {
            selector,
            kb: Mutex::new(None),
        }
    }

    /// Run `f` against the keyboard, (re)opening it on demand. A HID error
    /// drops the handle so the next call reconnects.
    pub fn with_keyboard<T>(&self, f: impl FnOnce(&mut Keyboard) -> Result<T>) -> Result<T> {
        let mut guard = self.kb.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            let kb = match &self.selector {
                Some(s) => Keyboard::open_matching(s)?,
                None => Keyboard::open_first()?,
            };
            tracing::info!(model = kb.summary().model, "opened keyboard");
            *guard = Some(kb);
        }
        let kb = guard.as_mut().expect("just opened");
        match f(kb) {
            Err(Error::Hid(e)) => {
                tracing::warn!(error = %e, "hid error, dropping device handle");
                *guard = None;
                Err(Error::Hid(e))
            }
            other => other,
        }
    }

    pub fn call(&self, method: &str, params: Value) -> ApiResult {
        let p = Params(params);
        match method {
            "device.info" => self.with_keyboard(|kb| {
                let mut s = serde_json::to_value(kb.summary()).unwrap();
                s["active_profile"] = kb.active_profile().ok().into();
                Ok(s)
            }).map_err(Into::into),
            "device.keys" => self.with_keyboard(|kb| Ok(json!({
                "layout": kb.layout().name,
                "keys": kb.layout().keys.iter().map(|k| json!({
                    "id": k.id, "label": k.label, "x": k.x, "y": k.y, "w": k.w, "h": k.h, "cells": k.cells,
                })).collect::<Vec<_>>(),
            }))).map_err(Into::into),
            "profile.list" => self.with_keyboard(|kb| Ok(serde_json::to_value(kb.profiles()?).unwrap())).map_err(Into::into),
            "profile.activate" => {
                let slot = p.u8("slot")?;
                self.with_keyboard(|kb| {
                    kb.activate_profile(slot)?;
                    Ok(json!({"active_profile": slot}))
                }).map_err(Into::into)
            }
            "rgb.fill" => {
                let c = p.color("color")?;
                self.with_keyboard(|kb| { kb.fill(c)?; Ok(json!({"ok": true})) }).map_err(Into::into)
            }
            "rgb.set_key" => {
                let key = p.string("key")?;
                let c = p.color("color")?;
                self.with_keyboard(|kb| { kb.set_key(&key, c)?; Ok(json!({"ok": true})) }).map_err(Into::into)
            }
            "rgb.set_keys" => {
                let map = p.0.get("colors").and_then(Value::as_object).ok_or_else(|| ApiError::invalid("colors must be an object"))?;
                let mut pairs = Vec::new();
                for (k, v) in map {
                    let s = v.as_str().ok_or_else(|| ApiError::invalid(format!("colour for {k} must be a string")))?;
                    pairs.push((k.clone(), Rgb::parse_hex(s).ok_or_else(|| ApiError::invalid(format!("bad colour {s:?}")))?));
                }
                self.with_keyboard(|kb| { kb.set_keys(&pairs)?; Ok(json!({"ok": true, "keys": pairs.len()})) }).map_err(Into::into)
            }
            "rgb.set_frame" => {
                let rows = p.0.get("rows").and_then(Value::as_array).ok_or_else(|| ApiError::invalid("rows must be an array"))?;
                let mut frame = Frame::default();
                for (r, row) in rows.iter().take(ROWS).enumerate() {
                    for (c, v) in row.as_array().into_iter().flatten().take(COLS).enumerate() {
                        if let Some(s) = v.as_str() {
                            frame.set(r, c, Rgb::parse_hex(s).ok_or_else(|| ApiError::invalid(format!("bad colour {s:?}")))?);
                        }
                    }
                }
                self.with_keyboard(|kb| { kb.set_frame(frame)?; Ok(json!({"ok": true})) }).map_err(Into::into)
            }
            "rgb.get_frame" => self.with_keyboard(|kb| Ok(frame_json(kb.frame()))).map_err(Into::into),
            "rgb.from_image" => {
                let img = if let Some(path) = p.opt_string("path") {
                    image_map::load_image(&PathBuf::from(expand_home(&path))).map_err(ApiError::from)?
                } else if let Some(b64) = p.opt_string("data_base64") {
                    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| ApiError::invalid(format!("base64: {e}")))?;
                    image_map::decode_image(&bytes).map_err(ApiError::from)?
                } else {
                    return Err(ApiError::invalid("need path or data_base64"));
                };
                let mut opts = ImageOptions::default();
                if let Some(m) = p.opt_string("mode") {
                    opts.mode = serde_json::from_value(Value::String(m)).map_err(|_| ApiError::invalid("mode must be cover|fit|stretch"))?;
                }
                if let Some(g) = p.0.get("gamma").and_then(Value::as_f64) { opts.gamma = g as f32; }
                if let Some(b) = p.0.get("brightness").and_then(Value::as_f64) { opts.brightness = b as f32; }
                self.with_keyboard(|kb| {
                    let frame = image_map::frame_from_image(&img, kb.layout(), &opts);
                    kb.set_frame(frame)?;
                    Ok(frame_json(kb.frame()))
                }).map_err(Into::into)
            }
            "rgb.release" => self.with_keyboard(|kb| { kb.release()?; Ok(json!({"ok": true})) }).map_err(Into::into),
            "rgb.brightness.get" => {
                let slot = p.opt_u8("slot")?;
                self.with_keyboard(|kb| {
                    let slot = slot.unwrap_or_else(|| kb.default_rgb_slot());
                    Ok(json!({"value": kb.brightness(slot)?, "slot": slot}))
                }).map_err(Into::into)
            }
            "rgb.brightness.set" => {
                let v = p.u8("value")?;
                let slot = p.opt_u8("slot")?;
                self.with_keyboard(|kb| {
                    let slot = slot.unwrap_or_else(|| kb.default_rgb_slot());
                    kb.set_brightness(v, slot)?;
                    Ok(json!({"value": v, "slot": slot}))
                }).map_err(Into::into)
            }
            "rgb.profile.get" => {
                let slot = p.opt_u8("slot")?;
                self.with_keyboard(|kb| {
                    let slot = slot.unwrap_or_else(|| kb.default_rgb_slot());
                    let mut v = frame_json(&kb.read_rgb_profile(slot)?);
                    v["slot"] = slot.into();
                    Ok(v)
                }).map_err(Into::into)
            }
            "rgb.profile.set" => {
                let rows = p.0.get("rows").and_then(Value::as_array).ok_or_else(|| ApiError::invalid("rows must be an array"))?;
                let mut frame = Frame::default();
                for (r, row) in rows.iter().take(ROWS).enumerate() {
                    for (c, v) in row.as_array().into_iter().flatten().take(COLS).enumerate() {
                        if let Some(s) = v.as_str() {
                            frame.set(r, c, Rgb::parse_hex(s).ok_or_else(|| ApiError::invalid(format!("bad colour {s:?}")))?);
                        }
                    }
                }
                let slot = p.opt_u8("slot")?;
                let brightness = p.opt_u8("brightness")?;
                self.with_keyboard(|kb| {
                    let slot = slot.unwrap_or_else(|| kb.default_rgb_slot());
                    kb.write_rgb_profile_with(&frame, brightness, slot)?;
                    Ok(json!({"ok": true, "slot": slot}))
                }).map_err(Into::into)
            }
            "rgb.save" => {
                let slot = p.opt_u8("slot")?;
                self.with_keyboard(|kb| {
                    let slot = slot.unwrap_or_else(|| kb.default_rgb_slot());
                    let frame = *kb.frame();
                    kb.write_rgb_profile(&frame, slot)?;
                    Ok(json!({"ok": true, "slot": slot}))
                }).map_err(Into::into)
            }
            "theme.apply" => {
                let dir = match p.opt_string("dir") {
                    Some(d) => PathBuf::from(expand_home(&d)),
                    None => theme::default_theme_dir().ok_or_else(|| ApiError::invalid("no XDG state dir; pass dir"))?,
                };
                let persist = p.0.get("persist").and_then(Value::as_bool).unwrap_or(false);
                let colors = theme::load_theme(&dir).map_err(ApiError::from)?;
                self.with_keyboard(|kb| {
                    let frame = theme::frame_from_theme(&colors, kb.layout());
                    kb.set_frame(frame)?;
                    if persist {
                        let slot = kb.default_rgb_slot();
                        kb.write_rgb_profile(&frame, slot)?;
                    }
                    Ok(json!({"ok": true, "theme_dir": dir, "colors": colors, "persisted": persist}))
                }).map_err(Into::into)
            }
            "settings.get" => {
                let slot = p.opt_u8("slot")?;
                self.with_keyboard(|kb| {
                    let slot = slot.map(Ok).unwrap_or_else(|| kb.active_profile())?;
                    let mut v = serde_json::to_value(kb.settings(slot)?).unwrap();
                    v["slot"] = slot.into();
                    Ok(v)
                }).map_err(Into::into)
            }
            "settings.set" => {
                let persist = p.0.get("persist").and_then(Value::as_bool).unwrap_or(false);
                let patch = p.0.clone();
                self.with_keyboard(|kb| {
                    let active = kb.active_profile()?;
                    let mut s = kb.settings(active)?;
                    if let Some(v) = patch.get("actuation_mm").and_then(Value::as_f64) { s.actuation_mm = v as f32; }
                    if let Some(v) = patch.get("rapid_trigger").and_then(Value::as_bool) { s.rapid_trigger = v; }
                    if let Some(v) = patch.get("rt_press_mm").and_then(Value::as_f64) { s.rt_press_mm = v as f32; }
                    match patch.get("rt_release_mm") {
                        Some(Value::Null) => s.rt_release_mm = None,
                        Some(v) => if let Some(f) = v.as_f64() { s.rt_release_mm = Some(f as f32) },
                        None => {}
                    }
                    if let Some(v) = patch.get("tachyon").and_then(Value::as_bool) { s.tachyon = v; }
                    kb.set_settings(&s, persist.then_some(active))?;
                    let mut out = serde_json::to_value(if persist { kb.settings(active)? } else { s }).unwrap();
                    out["persisted"] = persist.into();
                    out["slot"] = active.into();
                    Ok(out)
                }).map_err(Into::into)
            }
            "settings.global.get" => self.with_keyboard(|kb| Ok(serde_json::to_value(kb.global_settings()?).unwrap())).map_err(Into::into),
            "settings.global.set" => {
                let patch = p.0.clone();
                self.with_keyboard(|kb| {
                    let mut g = kb.global_settings()?;
                    let ps = g.power_saving.get_or_insert_with(Default::default);
                    if let Some(v) = patch.get("dim_after_minutes").and_then(Value::as_u64) { ps.dim_after_minutes = Some(v as u32); }
                    if let Some(v) = patch.get("off_after_minutes").and_then(Value::as_u64) { ps.off_after_minutes = Some(v as u32); }
                    ps.dim_after_minutes.get_or_insert(0);
                    ps.off_after_minutes.get_or_insert(0);
                    if let Some(v) = patch.get("numlock_inverted").and_then(Value::as_bool) { g.numlock_inverted = Some(v as u32); }
                    if let Some(v) = patch.get("debounce_filter").and_then(Value::as_bool) { g.debounce_filter = Some(v as u32); }
                    g.numlock_inverted.get_or_insert(0);
                    g.debounce_filter.get_or_insert(0);
                    kb.set_global_settings(&g)?;
                    Ok(serde_json::to_value(kb.global_settings()?).unwrap())
                }).map_err(Into::into)
            }
            "keymap.get" => {
                let layer = p.opt_u8("layer")?.unwrap_or(0);
                let slot = p.opt_u8("slot")?;
                self.with_keyboard(|kb| {
                    let slot = slot.map(Ok).unwrap_or_else(|| kb.active_profile())?;
                    let stored = kb.mapping(slot, layer)?;
                    let rows = stored.unwrap_or_else(|| kb.layout().default_mapping(layer));
                    Ok(mapping_json(kb, slot, layer, &rows, stored.is_some()))
                }).map_err(Into::into)
            }
            "keymap.set_key" => {
                let key = p.string("key")?;
                let code = p.keycode("code")?;
                let layer = p.opt_u8("layer")?.unwrap_or(0);
                let persist = p.0.get("persist").and_then(Value::as_bool).unwrap_or(false);
                self.with_keyboard(|kb| {
                    let slot = kb.active_profile()?;
                    let rows = kb.set_key_mapping(layer, &key, code, persist.then_some(slot))?;
                    Ok(mapping_json(kb, slot, layer, &rows, true))
                }).map_err(Into::into)
            }
            "keymap.set" => {
                let map = p.0.get("keys").and_then(Value::as_object).ok_or_else(|| ApiError::invalid("keys must be an object"))?;
                let layer = p.opt_u8("layer")?.unwrap_or(0);
                let persist = p.0.get("persist").and_then(Value::as_bool).unwrap_or(false);
                let mut pairs = Vec::new();
                for (k, v) in map {
                    let s = v.as_str().ok_or_else(|| ApiError::invalid(format!("code for {k} must be a string")))?;
                    pairs.push((k.clone(), keycode::parse(s).ok_or_else(|| ApiError::invalid(format!("unknown key code {s:?}")))?));
                }
                self.with_keyboard(|kb| {
                    let slot = kb.active_profile()?;
                    let mut rows = kb.mapping_or_default(slot, layer)?;
                    for (id, code) in &pairs {
                        let key = kb.layout().key(id).ok_or_else(|| Error::UnknownKey(id.clone()))?;
                        for c in &key.cells {
                            rows[c[0] as usize][c[1] as usize] = *code;
                        }
                    }
                    kb.set_mapping(layer, &rows, persist.then_some(slot))?;
                    Ok(mapping_json(kb, slot, layer, &rows, true))
                }).map_err(Into::into)
            }
            "keymap.reset" => {
                let layer = p.opt_u8("layer")?.unwrap_or(0);
                let persist = p.0.get("persist").and_then(Value::as_bool).unwrap_or(false);
                self.with_keyboard(|kb| {
                    let slot = kb.active_profile()?;
                    let rows = kb.layout().default_mapping(layer);
                    kb.set_mapping(layer, &rows, persist.then_some(slot))?;
                    Ok(mapping_json(kb, slot, layer, &rows, true))
                }).map_err(Into::into)
            }
            "keycodes.list" => Ok(json!({"codes": keycode::KEYCODES.iter().map(|(c, n)| json!({"code": c, "name": n})).collect::<Vec<_>>()})),
            "advanced_keys.get" => {
                let slot = p.opt_u8("slot")?;
                self.with_keyboard(|kb| {
                    let slot = slot.map(Ok).unwrap_or_else(|| kb.active_profile())?;
                    Ok(json!({"slot": slot, "entries": kb.advanced_keys(slot)?}))
                }).map_err(Into::into)
            }
            "advanced_keys.set_mod_tap" => {
                let key = p.string("key")?;
                let hold = p.keycode("hold")?;
                let tap = p.keycode("tap")?;
                self.with_keyboard(|kb| Ok(json!({"entries": kb.set_mod_tap(&key, tap, hold)?}))).map_err(Into::into)
            }
            "advanced_keys.set" => {
                let key = p.string("key")?;
                let kind = p.string("kind")?;
                let params = p.0.clone();
                self.with_keyboard(|kb| {
                    let entry = match kind.as_str() {
                        "mod_tap" => Keyboard::akc_mod_tap(Params(params.clone()).keycode("tap")?, Params(params.clone()).keycode("hold")?),
                        "toggle" => Keyboard::akc_toggle(Params(params.clone()).keycode("code")?),
                        "rappy_snappy" => kb.akc_rappy_snappy(&Params(params.clone()).string("other_key")?)?,
                        "socd" => kb.akc_socd(&Params(params.clone()).string("other_key")?, params.get("mode").and_then(Value::as_u64).unwrap_or(1) as u32)?,
                        "dks" => {
                            let names = params.get("codes").and_then(Value::as_array).ok_or_else(|| ApiError::invalid("dks needs codes"))?;
                            let mut codes = [0xffu8; 4];
                            for (i, n) in names.iter().take(4).enumerate() {
                                let s = n.as_str().unwrap_or("none");
                                codes[i] = keycode::parse(s).ok_or_else(|| ApiError::invalid(format!("unknown key code {s:?}")))?;
                            }
                            let masks = params.get("masks").and_then(Value::as_array).map(|m| {
                                let mut out = [0u8; 4];
                                for (i, v) in m.iter().take(4).enumerate() { out[i] = v.as_u64().unwrap_or(0) as u8; }
                                out
                            });
                            Keyboard::akc_dks(codes, masks)
                        }
                        other => return Err(ApiError::invalid(format!("unknown kind {other:?}")).into()),
                    };
                    Ok(json!({"entries": kb.set_advanced_key(&key, entry)?}))
                }).map_err(Into::into)
            }
            "actuation.keys.get" => {
                let slot = p.opt_u8("slot")?;
                self.with_keyboard(|kb| {
                    let slot = slot.map(Ok).unwrap_or_else(|| kb.active_profile())?;
                    let m: serde_json::Map<String, Value> = kb.per_key_actuation(slot)?.into_iter().map(|(k, mm)| (k, json!(mm))).collect();
                    Ok(json!({"slot": slot, "keys": m}))
                }).map_err(Into::into)
            }
            "actuation.keys.set" => {
                let map = p.0.get("keys").and_then(Value::as_object).ok_or_else(|| ApiError::invalid("keys must be an object"))?;
                let persist = p.0.get("persist").and_then(Value::as_bool).unwrap_or(false);
                let changes: Vec<(String, Option<f32>)> = map.iter().map(|(k, v)| (k.clone(), v.as_f64().map(|f| f as f32))).collect();
                self.with_keyboard(|kb| {
                    let slot = kb.active_profile()?;
                    let m: serde_json::Map<String, Value> = kb.set_per_key_actuation(&changes, persist.then_some(slot))?.into_iter().map(|(k, mm)| (k, json!(mm))).collect();
                    Ok(json!({"slot": slot, "keys": m, "persisted": persist}))
                }).map_err(Into::into)
            }
            "rapid_trigger.keys.get" => {
                let slot = p.opt_u8("slot")?;
                self.with_keyboard(|kb| {
                    let slot = slot.map(Ok).unwrap_or_else(|| kb.active_profile())?;
                    Ok(json!({"slot": slot, "keys": kb.rt_keys(slot)?}))
                }).map_err(Into::into)
            }
            "rapid_trigger.keys.set" => {
                let map = p.0.get("keys").and_then(Value::as_object).ok_or_else(|| ApiError::invalid("keys must be an object"))?;
                let persist = p.0.get("persist").and_then(Value::as_bool).unwrap_or(false);
                let changes: Vec<(String, bool)> = map.iter().map(|(k, v)| (k.clone(), v.as_bool().unwrap_or(false))).collect();
                self.with_keyboard(|kb| {
                    let slot = kb.active_profile()?;
                    Ok(json!({"slot": slot, "keys": kb.set_rt_keys(&changes, persist.then_some(slot))?, "persisted": persist}))
                }).map_err(Into::into)
            }
            "rgb.effects.get" => {
                let slot = p.opt_u8("slot")?;
                self.with_keyboard(|kb| {
                    let slot = slot.map(Ok).unwrap_or_else(|| kb.active_profile())?;
                    let core = kb.rgb_core(slot)?;
                    Ok(json!({"slot": slot, "brightness": core.payload.as_ref().and_then(|p| p.brightness), "layers": core.effects.map(|e| e.layers).unwrap_or_default()}))
                }).map_err(Into::into)
            }
            "rgb.effects.set" => {
                let layers: Vec<pb::EffectLayer> = serde_json::from_value(p.0.get("layers").cloned().unwrap_or(json!([]))).map_err(|e| ApiError::invalid(format!("layers: {e}")))?;
                let brightness = p.opt_u8("brightness")?;
                let persist = p.0.get("persist").and_then(Value::as_bool).unwrap_or(false);
                self.with_keyboard(|kb| {
                    let slot = kb.active_profile()?;
                    let core = kb.set_rgb_core(Some(pb::Effects { layers }), brightness, persist.then_some(slot))?;
                    Ok(json!({"slot": slot, "brightness": core.payload.as_ref().and_then(|p| p.brightness), "layers": core.effects.map(|e| e.layers).unwrap_or_default(), "persisted": persist}))
                }).map_err(Into::into)
            }
            "settings.device.get" => self.with_keyboard(|kb| Ok(json!({"compat_mode": kb.compat_mode()?, "gamepad_mode": kb.gamepad_mode()?, "raw": kb.device_config()?}))).map_err(Into::into),
            "settings.device.set" => {
                let compat = p.0.get("compat_mode").and_then(Value::as_bool);
                let gamepad = p.0.get("gamepad_mode").and_then(Value::as_bool);
                self.with_keyboard(|kb| {
                    if let Some(c) = compat { kb.set_compat_mode(c)?; }
                    if let Some(g) = gamepad { kb.set_gamepad_mode(g)?; }
                    Ok(json!({"ok": true, "restarting": compat.is_some() || gamepad.is_some()}))
                }).map_err(Into::into)
            }
            "advanced_keys.remove" => {
                let key = p.string("key")?;
                self.with_keyboard(|kb| Ok(json!({"entries": kb.remove_advanced_key(&key)?}))).map_err(Into::into)
            }
            "profile.rename" => {
                let slot = p.u8("slot")?;
                let name = p.opt_string("name");
                let color = match p.opt_string("color") {
                    Some(c) => Some(Rgb::parse_hex(&c).ok_or_else(|| ApiError::invalid("bad colour"))?),
                    None => None,
                };
                self.with_keyboard(|kb| Ok(serde_json::to_value(kb.set_profile_metadata(slot, name.as_deref(), color)?).unwrap())).map_err(Into::into)
            }
            "profile.delete" => {
                let slot = p.u8("slot")?;
                if !p.0.get("confirm").and_then(Value::as_bool).unwrap_or(false) {
                    return Err(ApiError::invalid("deleting a profile is irreversible; pass confirm=true"));
                }
                self.with_keyboard(|kb| { kb.delete_profile(slot)?; Ok(json!({"deleted": slot})) }).map_err(Into::into)
            }
            "backup.create" => self.with_keyboard(|kb| Ok(json!({"path": backup::save(kb)?}))).map_err(Into::into),
            "raw.command" => {
                let name = p.string("command")?;
                let cmd = Command::parse(&name).ok_or_else(|| ApiError::invalid(format!("unknown command {name:?}")))?;
                let confirm = p.0.get("confirm").and_then(Value::as_bool).unwrap_or(false);
                if cmd.is_dangerous() && !confirm {
                    return Err(ApiError::invalid(format!("{cmd} resets or reflashes the keyboard; pass confirm=true")));
                }
                let mut params = [0u8; 4];
                for (i, v) in p.0.get("params").and_then(Value::as_array).into_iter().flatten().take(4).enumerate() {
                    params[i] = v.as_u64().filter(|x| *x < 256).ok_or_else(|| ApiError::invalid("params must be bytes"))? as u8;
                }
                self.with_keyboard(|kb| {
                    let r = kb.transport().command(cmd, params)?;
                    Ok(json!({
                        "command": cmd.name(),
                        "status": r.status(),
                        "status_name": wooting_proto::status_name(r.status()),
                        "data": r.data(),
                        "raw_hex": r.raw.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "),
                    }))
                }).map_err(Into::into)
            }
            other => Err(ApiError::method_not_found(other)),
        }
    }
}

fn mapping_json(kb: &Keyboard, slot: u8, layer: u8, rows: &[[u8; COLS]; ROWS], stored: bool) -> Value {
    let keys: serde_json::Map<String, Value> = kb
        .mapping_by_key(rows)
        .into_iter()
        .map(|(id, code, name)| (id, json!({"code": code, "name": name, "label": keycode::label(code)})))
        .collect();
    json!({"slot": slot, "layer": layer, "stored": stored, "keys": keys, "rows": rows.iter().map(|r| r.to_vec()).collect::<Vec<_>>()})
}

fn frame_json(f: &Frame) -> Value {
    json!({"rows": f.0.iter().map(|r| r.iter().map(|c| c.to_hex()).collect::<Vec<_>>()).collect::<Vec<_>>()})
}

fn expand_home(p: &str) -> String {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map(|h| h.join(rest).to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string()),
        None => p.to_string(),
    }
}

struct Params(Value);

impl Params {
    fn string(&self, k: &str) -> std::result::Result<String, ApiError> {
        self.opt_string(k).ok_or_else(|| ApiError::invalid(format!("missing string param {k:?}")))
    }
    fn opt_string(&self, k: &str) -> Option<String> {
        self.0.get(k).and_then(Value::as_str).map(str::to_owned)
    }
    fn opt_u8(&self, k: &str) -> std::result::Result<Option<u8>, ApiError> {
        match self.0.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.u8(k).map(Some),
        }
    }
    fn u8(&self, k: &str) -> std::result::Result<u8, ApiError> {
        self.0.get(k).and_then(Value::as_u64).filter(|v| *v < 256).map(|v| v as u8)
            .ok_or_else(|| ApiError::invalid(format!("param {k:?} must be an integer 0-255")))
    }
    fn keycode(&self, k: &str) -> std::result::Result<u8, ApiError> {
        let s = self.string(k)?;
        keycode::parse(&s).ok_or_else(|| ApiError::invalid(format!("{k}: unknown key code {s:?}; see keycodes.list")))
    }
    fn color(&self, k: &str) -> std::result::Result<Rgb, ApiError> {
        let s = self.string(k)?;
        Rgb::parse_hex(&s).ok_or_else(|| ApiError::invalid(format!("{k}: bad colour {s:?}, expected #rrggbb")))
    }
}

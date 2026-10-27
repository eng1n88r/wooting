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
use wooting_proto::{Command, Frame, Rgb, COLS, ROWS};

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
        MethodInfo { name: "rgb.save", description: "Persist the current direct-mode frame into an RGB profile slot (default: active profile) so it survives unplugging. Writes flash.", params: obj(json!({"slot": {"type": "integer", "minimum": 0, "description": "RGB profile slot; default = active profile"}}), &[]) },
        MethodInfo { name: "theme.apply", description: "Colour the keyboard from an Omarchy theme directory (default: the current theme). persist=true also saves it to the keyboard.", params: obj(json!({"dir": {"type": "string"}, "persist": {"type": "boolean"}}), &[]) },
        MethodInfo { name: "settings.global.get", description: "Global settings (RGB power saving timers).", params: obj(json!({}), &[]) },
        MethodInfo { name: "keymap.get", description: "Raw key mapping matrix of the active profile (Wooting key codes, 255 = none).", params: obj(json!({}), &[]) },
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
            "settings.global.get" => self.with_keyboard(|kb| Ok(serde_json::to_value(kb.global_settings()?).unwrap())).map_err(Into::into),
            "keymap.get" => self.with_keyboard(|kb| {
                let m = kb.mapping()?;
                Ok(json!({"rows": m.rows.iter().map(|r| r.codes.clone()).collect::<Vec<_>>()}))
            }).map_err(Into::into),
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
    fn color(&self, k: &str) -> std::result::Result<Rgb, ApiError> {
        let s = self.string(k)?;
        Rgb::parse_hex(&s).ok_or_else(|| ApiError::invalid(format!("{k}: bad colour {s:?}, expected #rrggbb")))
    }
}

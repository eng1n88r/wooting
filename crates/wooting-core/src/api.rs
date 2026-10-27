//! The one operation set shared by the daemon's HTTP and socket servers,
//! the MCP server and the CLI. Every surface ends up in [`Api::call`].
//!
//! Profile edits go through a per-slot [`Draft`]: the change is applied to
//! the keyboard's RAM immediately and, when `persist` is true (the default
//! for CLI/MCP/HTTP callers), also written to flash. The GUI passes
//! `persist: false` and uses `profile.save` / `profile.discard` /
//! `profile.undo` / `profile.redo` like Wootility's "Save to Keyboard".

use crate::{
    backup,
    draft::{Draft, DraftStatus, ProfileData, Rows},
    image_map,
    image_map::ImageOptions,
    theme, Error, Keyboard, Result,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf, sync::Mutex};
use wooting_proto::{gamepad, keycode, pb, Command, Frame, Layout, Rgb, COLS, ROWS};

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

const PERSIST_DOC: &str = "persist (default true): also write flash. With false the change is live in RAM only and tracked in the profile draft (see profile.save).";

pub fn methods() -> Vec<MethodInfo> {
    let color = json!({"type": "string", "description": "Colour as #rrggbb"});
    let slot = json!({"type": "integer", "minimum": 0, "description": "Profile slot; default = active profile"});
    let persist = json!({"type": "boolean", "description": PERSIST_DOC});
    let layer = json!({"type": "integer", "minimum": 0, "maximum": 3, "description": "0 = main, 1-3 = Fn layers"});
    let obj = |props: Value, required: &[&str]| json!({"type": "object", "properties": props, "required": required, "additionalProperties": false});
    vec![
        MethodInfo { name: "device.info", description: "Model, serial, firmware, active profile, direct-RGB state and the active profile's draft status (dirty/undo/redo).", params: obj(json!({}), &[]) },
        MethodInfo { name: "device.keys", description: "List key ids (for rgb.set_keys, keymap.*, actuation.keys.*) with labels and positions.", params: obj(json!({}), &[]) },
        MethodInfo { name: "device.analog", description: "Current analog travel (0-255) of every key; useful to see what is pressed.", params: obj(json!({}), &[]) },
        MethodInfo { name: "profile.list", description: "List keyboard profiles (slot, name, colour, active, dirty).", params: obj(json!({}), &[]) },
        MethodInfo { name: "profile.activate", description: "Switch the keyboard to profile slot N (0-based). Fails if the current profile has unsaved draft edits unless discard=true or save=true.", params: obj(json!({"slot": {"type": "integer", "minimum": 0}, "discard": {"type": "boolean"}, "save": {"type": "boolean"}}), &["slot"]) },
        MethodInfo { name: "profile.rename", description: "Rename a profile slot and/or set its badge colour.", params: obj(json!({"slot": slot, "name": {"type": "string"}, "color": color, "persist": persist}), &[]) },
        MethodInfo { name: "profile.delete", description: "Delete an onboard profile slot. Irreversible; needs confirm=true.", params: obj(json!({"slot": {"type": "integer"}, "confirm": {"type": "boolean"}}), &["slot"]) },
        MethodInfo { name: "profile.create", description: "Create a new onboard profile in the first free slot (max 4) as a copy of another slot (default: factory-like copy of slot 0) with a new name. Writes flash.", params: obj(json!({"name": {"type": "string"}, "color": color, "from_slot": {"type": "integer"}}), &["name"]) },
        MethodInfo { name: "profile.swap", description: "Swap two profile slots (Wootility's 'Set as Default' moves a profile into slot 0 this way). Writes flash.", params: obj(json!({"a": {"type": "integer"}, "b": {"type": "integer"}}), &["a", "b"]) },
        MethodInfo { name: "profile.export", description: "Full profile data of a slot as JSON (for backups / profile.import).", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "profile.import", description: "Write exported profile data into a slot. Writes flash.", params: obj(json!({"slot": {"type": "integer"}, "data": {"type": "object"}, "name": {"type": "string"}}), &["slot", "data"]) },
        MethodInfo { name: "profile.status", description: "Draft status of a slot: dirty, can_undo, can_redo.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "profile.save", description: "Write the slot's draft (pending live edits) to flash.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "profile.discard", description: "Drop pending edits and reload the slot from flash.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "profile.undo", description: "Undo the last draft edit (live).", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "profile.redo", description: "Redo the last undone draft edit (live).", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "rgb.fill", description: "Light every key with one colour (volatile direct mode; the keyboard's own lighting resumes after rgb.release).", params: obj(json!({"color": color}), &["color"]) },
        MethodInfo { name: "rgb.set_key", description: "Set one key's colour by id (volatile direct mode).", params: obj(json!({"key": {"type": "string"}, "color": color}), &["key", "color"]) },
        MethodInfo { name: "rgb.set_keys", description: "Set many keys at once: {\"colors\": {\"esc\": \"#ff0000\", \"space\": \"#00ff00\"}} (volatile direct mode).", params: obj(json!({"colors": {"type": "object", "additionalProperties": color}}), &["colors"]) },
        MethodInfo { name: "rgb.set_frame", description: "Set the whole LED matrix: rows is 6 rows of up to 21 colours (#rrggbb or null) (volatile direct mode).", params: obj(json!({"rows": {"type": "array", "items": {"type": "array", "items": {"type": ["string", "null"]}}}}), &["rows"]) },
        MethodInfo { name: "rgb.get_frame", description: "Current direct-mode frame as 6 rows of #rrggbb.", params: obj(json!({}), &[]) },
        MethodInfo { name: "rgb.from_image", description: "Paint the keyboard with an image (file path or base64 data). mode: cover|fit|stretch, gamma default 0.8. Volatile unless rgb.save follows.", params: obj(json!({"path": {"type": "string"}, "data_base64": {"type": "string"}, "mode": {"type": "string", "enum": ["cover", "fit", "stretch"]}, "gamma": {"type": "number"}, "brightness": {"type": "number"}}), &[]) },
        MethodInfo { name: "rgb.release", description: "Leave direct mode and return control of the LEDs to the keyboard's own lighting.", params: obj(json!({}), &[]) },
        MethodInfo { name: "rgb.save", description: "Store the current direct-mode frame as the static colours of a profile slot (default: active). Writes flash.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "rgb.profile.get", description: "Static colours stored in a profile slot (default: active), 6 rows of #rrggbb.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "rgb.profile.set", description: "Set stored static colours: whole rows (6 x 21 #rrggbb) and/or per key {\"colors\": {\"esc\": \"#ff0000\"}}, plus brightness.", params: obj(json!({"rows": {"type": "array"}, "colors": {"type": "object", "additionalProperties": color}, "brightness": {"type": "integer"}, "slot": slot, "persist": persist}), &[]) },
        MethodInfo { name: "rgb.brightness.get", description: "Stored brightness (0-255) of a profile slot.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "rgb.brightness.set", description: "Set stored brightness (0-255).", params: obj(json!({"value": {"type": "integer", "minimum": 0, "maximum": 255}, "slot": slot, "persist": persist}), &["value"]) },
        MethodInfo { name: "rgb.effects.get", description: "Effect layers of a profile slot. Layer kinds: wave, scan, breath, cycle, ripple, trail, touch, jelly, aoe, mixing, rt_display, noise, spiral; [] = static.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "rgb.effects.set", description: "Replace the effect layers, e.g. {\"layers\": [{\"wave\": {\"look\": 0, \"speed\": 16, \"direction\": 0}}]} ([] = static). The first layer is the base effect.", params: obj(json!({"layers": {"type": "array"}, "slot": slot, "persist": persist}), &["layers"]) },
        MethodInfo { name: "theme.apply", description: "Colour the keyboard from an Omarchy theme directory (default: the current theme) in direct mode; persist=true also stores it as the active profile's static colours.", params: obj(json!({"dir": {"type": "string"}, "persist": {"type": "boolean"}}), &[]) },
        MethodInfo { name: "settings.get", description: "Actuation point, rapid trigger (incl. continuous), Tachyon of a profile slot.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "settings.set", description: "Change actuation (mm, 0.1-4.0), rapid trigger (on/off for all keys, press/release sensitivity in mm, continuous_rt) and Tachyon.", params: obj(json!({"actuation_mm": {"type": "number"}, "rapid_trigger": {"type": "boolean"}, "rt_press_mm": {"type": "number"}, "rt_release_mm": {"type": ["number", "null"], "description": "null = same as press"}, "continuous_rt": {"type": "boolean"}, "tachyon": {"type": "boolean"}, "slot": slot, "persist": persist}), &[]) },
        MethodInfo { name: "actuation.keys.get", description: "Per-key actuation overrides as {key_id: mm}.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "actuation.keys.set", description: "Set per-key actuation overrides: {\"keys\": {\"a\": 2.5, \"s\": null}} (null removes the override).", params: obj(json!({"keys": {"type": "object", "additionalProperties": {"type": ["number", "null"]}}, "slot": slot, "persist": persist}), &["keys"]) },
        MethodInfo { name: "rapid_trigger.keys.get", description: "Keys with rapid trigger enabled individually.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "rapid_trigger.keys.set", description: "Enable/disable rapid trigger per key: {\"keys\": {\"w\": true, \"a\": false}}.", params: obj(json!({"keys": {"type": "object", "additionalProperties": {"type": "boolean"}}, "slot": slot, "persist": persist}), &["keys"]) },
        MethodInfo { name: "settings.global.get", description: "Device-wide settings: RGB power saving timers, inverted Num Lock indicator, debounce filter.", params: obj(json!({}), &[]) },
        MethodInfo { name: "settings.global.set", description: "Set device-wide settings: minutes until lighting dims / turns off (0 = never), numlock_inverted, debounce_filter.", params: obj(json!({"dim_after_minutes": {"type": "integer"}, "off_after_minutes": {"type": "integer"}, "numlock_inverted": {"type": "boolean"}, "debounce_filter": {"type": "boolean"}}), &[]) },
        MethodInfo { name: "settings.device.get", description: "Hardware flags: compatibility mode (6KRO), gamepad mode, gamepad polling rate (250/1000 Hz) and controller type (xbox/classic).", params: obj(json!({}), &[]) },
        MethodInfo { name: "settings.device.set", description: "Set compatibility mode (6KRO), gamepad mode, gamepad polling rate or controller type. The keyboard restarts; gamepad mode / controller type change its USB identity (PID 0x1320 Xbox, 0x1321 Classic).", params: obj(json!({"compat_mode": {"type": "boolean"}, "gamepad_mode": {"type": "boolean"}, "polling_hz": {"type": "integer", "enum": [250, 1000]}, "controller": {"type": "string", "enum": ["xbox", "classic"]}}), &[]) },
        MethodInfo { name: "gamepad.get", description: "Gamepad setup of a profile: buttons mapped onto keys ({key_id: button}), override (mapped keys stop typing), keyboard_keys (mapped keys still type), analog curve (4 [x,y] points, 0..255; first y = anti-deadzone), preset name if the curve matches one, angle_enabled/angle (0..100 = 0..90°), square, snappy; plus the device flags.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "gamepad.set", description: "Map controller buttons onto keys: {\"keys\": {\"z\": \"a\", \"x\": null}} (null removes), and/or set override / keyboard_keys. Button names: gamepad.buttons.", params: obj(json!({"keys": {"type": "object", "additionalProperties": {"type": ["string", "null"]}}, "override": {"type": "boolean"}, "keyboard_keys": {"type": "boolean"}, "slot": slot, "persist": persist}), &[]) },
        MethodInfo { name: "gamepad.curve.set", description: "Analog curve and joystick options: preset (linear|aggro|slow|smooth|steep|instant|turbo) or points [[x,y]×4], deadzone (0..255, y of the first point), angle_enabled, angle (0..100), square, snappy.", params: obj(json!({"preset": {"type": "string"}, "points": {"type": "array", "items": {"type": "array", "items": {"type": "integer"}}}, "deadzone": {"type": "integer"}, "angle_enabled": {"type": "boolean"}, "angle": {"type": "integer"}, "square": {"type": "boolean"}, "snappy": {"type": "boolean"}, "slot": slot, "persist": persist}), &[]) },
        MethodInfo { name: "gamepad.buttons", description: "Controller button names, labels and codes, plus the curve presets.", params: obj(json!({}), &[]) },
        MethodInfo { name: "settings.reset", description: "Reset ALL keyboard settings to factory (ResetSettings). Irreversible; needs confirm=true.", params: obj(json!({"confirm": {"type": "boolean"}}), &[]) },
        MethodInfo { name: "keymap.get", description: "Key mapping of a layer as {key_id: {code, name, label}} plus the raw matrix; stored=false means the factory default is in use.", params: obj(json!({"layer": layer, "slot": slot}), &[]) },
        MethodInfo { name: "keymap.set_key", description: "Remap one key: code is a key name from keycodes.list (e.g. \"esc\", \"f5\", \"lctrl\", \"mouse_left\", \"none\").", params: obj(json!({"key": {"type": "string"}, "code": {"type": "string"}, "layer": layer, "slot": slot, "persist": persist}), &["key", "code"]) },
        MethodInfo { name: "keymap.set", description: "Remap several keys at once: {\"keys\": {\"capslock\": \"esc\", \"esc\": \"grave\"}}.", params: obj(json!({"keys": {"type": "object", "additionalProperties": {"type": "string"}}, "layer": layer, "slot": slot, "persist": persist}), &["keys"]) },
        MethodInfo { name: "keymap.set_layer", description: "Replace a whole layer with rows (6 x 21 codes or names) or a preset: default|function|dvorak|colemak|workman|1337|abc, or null to make the layer follow the factory default.", params: obj(json!({"layer": layer, "rows": {"type": ["array", "null"]}, "preset": {"type": "string"}, "slot": slot, "persist": persist}), &["layer"]) },
        MethodInfo { name: "keymap.reset", description: "Restore the factory mapping of a layer.", params: obj(json!({"layer": layer, "slot": slot, "persist": persist}), &[]) },
        MethodInfo { name: "keycodes.list", description: "All key code names accepted by keymap.* and advanced_keys.*.", params: obj(json!({}), &[]) },
        MethodInfo { name: "advanced_keys.get", description: "Advanced keys of a profile slot: mod_tap {tap,hold}, toggle {tap=code}, rappy_snappy {other_key}, socd {other_key,mode}, dks {dks_codes,dks_masks}.", params: obj(json!({"slot": slot}), &[]) },
        MethodInfo { name: "advanced_keys.set", description: "Add or replace an advanced key. kind=mod_tap {tap,hold} | toggle {code} | rappy_snappy {other_key} | socd {other_key, mode} | dks {codes:[4 names], masks:[4 ints]?}.", params: obj(json!({"key": {"type": "string"}, "kind": {"type": "string", "enum": ["mod_tap", "toggle", "rappy_snappy", "socd", "dks"]}, "tap": {"type": "string"}, "hold": {"type": "string"}, "code": {"type": "string"}, "other_key": {"type": "string"}, "mode": {"type": "integer"}, "codes": {"type": "array", "items": {"type": "string"}}, "masks": {"type": "array", "items": {"type": "integer"}}, "slot": slot, "persist": persist}), &["key", "kind"]) },
        MethodInfo { name: "advanced_keys.set_mod_tap", description: "Shortcut for advanced_keys.set kind=mod_tap.", params: obj(json!({"key": {"type": "string"}, "hold": {"type": "string"}, "tap": {"type": "string"}, "slot": slot, "persist": persist}), &["key", "hold", "tap"]) },
        MethodInfo { name: "advanced_keys.remove", description: "Remove the advanced-key entry from a key.", params: obj(json!({"key": {"type": "string"}, "slot": slot, "persist": persist}), &["key"]) },
        MethodInfo { name: "links.list", description: "Linked profiles (daemon only): rules that switch the profile when a Hyprland window gets focus, from ~/.config/wooting/links.toml.", params: obj(json!({}), &[]) },
        MethodInfo { name: "links.add", description: "Add a linked-profile rule: class and/or title substring → profile name or slot.", params: obj(json!({"class": {"type": "string"}, "title": {"type": "string"}, "profile": {"type": "string"}}), &["profile"]) },
        MethodInfo { name: "links.remove", description: "Remove rule by index.", params: obj(json!({"index": {"type": "integer"}}), &["index"]) },
        MethodInfo { name: "links.default", description: "Profile to use when no rule matches (omit to clear).", params: obj(json!({"profile": {"type": "string"}}), &[]) },
        MethodInfo { name: "backup.create", description: "Dump every readable profile/setting to a JSON file and return its path.", params: obj(json!({}), &[]) },
        MethodInfo { name: "raw.command", description: "Send a raw feature command by name or id with up to 4 byte params. Dangerous commands need confirm=true.", params: obj(json!({"command": {"type": "string"}, "params": {"type": "array", "items": {"type": "integer"}, "maxItems": 4}, "confirm": {"type": "boolean"}}), &["command"]) },
    ]
}

/// Owns the keyboard and serialises access to it.
pub struct Api {
    selector: Option<String>,
    kb: Mutex<Option<Keyboard>>,
    drafts: Mutex<HashMap<u8, Draft>>,
}

impl Api {
    pub fn new(selector: Option<String>) -> Api {
        Api {
            selector,
            kb: Mutex::new(None),
            drafts: Mutex::new(HashMap::new()),
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

    /// Current state of a slot: its draft if one exists, otherwise flash.
    fn view(&self, slot: Option<u8>) -> Result<(u8, ProfileData, Layout, DraftStatus)> {
        self.with_keyboard(|kb| {
            let slot = slot.map(Ok).unwrap_or_else(|| kb.active_profile())?;
            let mut drafts = self.drafts.lock().unwrap_or_else(|p| p.into_inner());
            let d = match drafts.get(&slot) {
                Some(d) => d,
                None => {
                    let d = Draft::load(kb, slot)?;
                    drafts.entry(slot).or_insert(d)
                }
            };
            Ok((slot, d.data.clone(), kb.layout().clone(), d.status()))
        })
    }

    /// Apply an edit to a slot's draft (live) and, with `persist`, save it.
    fn mutate(
        &self,
        slot: Option<u8>,
        persist: bool,
        f: impl FnOnce(&mut ProfileData, &Layout) -> Result<()>,
    ) -> Result<(u8, ProfileData, Layout, DraftStatus)> {
        self.with_keyboard(|kb| {
            let slot = slot.map(Ok).unwrap_or_else(|| kb.active_profile())?;
            let layout = kb.layout().clone();
            let mut drafts = self.drafts.lock().unwrap_or_else(|p| p.into_inner());
            let d = match drafts.entry(slot) {
                std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::hash_map::Entry::Vacant(e) => e.insert(Draft::load(kb, slot)?),
            };
            d.edit(kb, |data| f(data, &layout))?;
            if persist {
                d.save(kb)?;
            }
            Ok((slot, d.data.clone(), layout, d.status()))
        })
    }

    fn draft_op(
        &self,
        slot: Option<u8>,
        op: fn(&mut Draft, &Keyboard) -> Result<bool>,
    ) -> Result<(u8, bool, DraftStatus)> {
        self.with_keyboard(|kb| {
            let slot = slot.map(Ok).unwrap_or_else(|| kb.active_profile())?;
            let mut drafts = self.drafts.lock().unwrap_or_else(|p| p.into_inner());
            let d = match drafts.entry(slot) {
                std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::hash_map::Entry::Vacant(e) => e.insert(Draft::load(kb, slot)?),
            };
            let changed = op(d, kb)?;
            Ok((slot, changed, d.status()))
        })
    }

    fn forget_draft(&self, slot: u8) {
        self.drafts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&slot);
    }

    pub fn call(&self, method: &str, params: Value) -> ApiResult {
        let p = Params(params);
        let persist = p.bool("persist").unwrap_or(true);
        if !persist {
            tracing::info!(method, params = %p.0, "draft edit");
        }
        let slot = p.opt_u8("slot")?;
        match method {
            // ----- device ---------------------------------------------------
            "device.info" => self.with_keyboard(|kb| {
                let mut s = serde_json::to_value(kb.summary()).unwrap();
                let active = kb.active_profile().ok();
                s["active_profile"] = active.into();
                let drafts = self.drafts.lock().unwrap_or_else(|p| p.into_inner());
                s["draft"] = active.and_then(|a| drafts.get(&a)).map(|d| serde_json::to_value(d.status()).unwrap()).unwrap_or(Value::Null);
                s["dirty_slots"] = drafts.values().filter(|d| d.status().dirty).map(|d| d.slot).collect::<Vec<_>>().into();
                Ok(s)
            }).map_err(Into::into),
            "device.keys" => self.with_keyboard(|kb| Ok(json!({
                "layout": kb.layout().name,
                "keys": kb.layout().keys.iter().map(|k| json!({
                    "id": k.id, "label": k.label, "x": k.x, "y": k.y, "w": k.w, "h": k.h, "cells": k.cells,
                })).collect::<Vec<_>>(),
            }))).map_err(Into::into),
            "device.analog" => self.with_keyboard(|kb| {
                let v = kb.analog_values()?;
                let mut keys = serde_json::Map::new();
                for key in &kb.layout().keys {
                    let c = key.cells[0];
                    let val = v.rows.get(c[0] as usize).and_then(|r| r.values.get(c[1] as usize)).copied().unwrap_or(0);
                    if val > 0 {
                        keys.insert(key.id.clone(), json!(val));
                    }
                }
                Ok(json!({"pressed": keys, "rows": v.rows.iter().map(|r| r.values.clone()).collect::<Vec<_>>()}))
            }).map_err(Into::into),
            // ----- profiles -------------------------------------------------
            "profile.list" => self.with_keyboard(|kb| {
                let mut list = kb.profiles()?;
                let drafts = self.drafts.lock().unwrap_or_else(|p| p.into_inner());
                let out: Vec<Value> = list.iter_mut().map(|p| {
                    let mut v = serde_json::to_value(&p).unwrap();
                    if let Some(d) = drafts.get(&p.slot) {
                        v["name"] = d.data.meta.name.clone().into();
                        v["dirty"] = d.status().dirty.into();
                    } else {
                        v["dirty"] = false.into();
                    }
                    v
                }).collect();
                Ok(Value::Array(out))
            }).map_err(Into::into),
            "profile.activate" => {
                let target = p.u8("slot")?;
                let discard = p.bool("discard").unwrap_or(false);
                let save = p.bool("save").unwrap_or(false);
                self.with_keyboard(|kb| {
                    let current = kb.active_profile()?;
                    let mut drafts = self.drafts.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(d) = drafts.get_mut(&current) {
                        if d.status().dirty {
                            if save {
                                d.save(kb)?;
                            } else if discard {
                                d.discard(kb)?;
                            } else {
                                return Err(Error::Other(format!("profile {current} has unsaved edits; pass save=true or discard=true")));
                            }
                        }
                    }
                    kb.activate_profile(target)?;
                    Ok(json!({"active_profile": target}))
                }).map_err(Into::into)
            }
            "profile.rename" => {
                let name = p.opt_string("name");
                let color = p.opt_color("color")?;
                let (slot, data, _, st) = self.mutate(slot, persist, |d, _| { d.rename(name.as_deref(), color); Ok(()) })?;
                Ok(json!({"slot": slot, "name": data.meta.name, "draft": st}))
            }
            "profile.delete" => {
                let slot = p.u8("slot")?;
                if !p.bool("confirm").unwrap_or(false) {
                    return Err(ApiError::invalid("deleting a profile is irreversible; pass confirm=true"));
                }
                self.forget_draft(slot);
                self.with_keyboard(|kb| { kb.delete_profile(slot)?; Ok(json!({"deleted": slot})) }).map_err(Into::into)
            }
            "profile.create" => {
                let name = p.string("name")?;
                let color = p.opt_color("color")?;
                let from = p.opt_u8("from_slot")?.unwrap_or(0);
                self.with_keyboard(|kb| {
                    let count = kb.profiles()?.len() as u8;
                    if count >= 4 {
                        return Err(Error::Other("all 4 onboard slots are in use".into()));
                    }
                    let mut data = kb.read_profile(from)?;
                    data.rename(Some(&name), color);
                    kb.create_profile(count, &data)?;
                    Ok(json!({"slot": count, "name": name}))
                }).map_err(Into::into)
            }
            "profile.swap" => {
                let a = p.u8("a")?;
                let b = p.u8("b")?;
                self.forget_draft(a);
                self.forget_draft(b);
                self.with_keyboard(|kb| {
                    let da = kb.read_profile(a)?;
                    let db = kb.read_profile(b)?;
                    kb.write_profile(&db, crate::draft::WriteTarget::Flash(a))?;
                    kb.write_profile(&da, crate::draft::WriteTarget::Flash(b))?;
                    let active = kb.active_profile()?;
                    if active == a || active == b {
                        kb.cmd(Command::ReloadProfile, [0, 0, 0, active])?;
                    }
                    Ok(json!({"swapped": [a, b]}))
                }).map_err(Into::into)
            }
            "profile.export" => {
                let (slot, data, _, _) = self.view(slot)?;
                Ok(json!({"slot": slot, "format": "wooting-profile-v1", "data": data}))
            }
            "profile.import" => {
                let slot = p.u8("slot")?;
                let mut data: ProfileData = serde_json::from_value(p.0.get("data").cloned().unwrap_or(Value::Null)).map_err(|e| ApiError::invalid(format!("data: {e}")))?;
                if let Some(n) = p.opt_string("name") {
                    data.meta.name = n;
                }
                self.forget_draft(slot);
                self.with_keyboard(|kb| {
                    if kb.profile_metadata(slot)?.is_none() {
                        kb.create_profile(slot, &data)?;
                    } else {
                        kb.write_profile(&data, crate::draft::WriteTarget::Flash(slot))?;
                        if kb.active_profile()? == slot {
                            kb.cmd(Command::ReloadProfile, [0, 0, 0, slot])?;
                        }
                    }
                    Ok(json!({"slot": slot, "name": data.meta.name}))
                }).map_err(Into::into)
            }
            "profile.status" => {
                let (slot, _, _, st) = self.view(slot)?;
                Ok(json!({"slot": slot, "draft": st}))
            }
            "profile.save" => {
                let (slot, _, st) = self.draft_op(slot, |d, kb| { d.save(kb)?; Ok(true) })?;
                Ok(json!({"slot": slot, "draft": st}))
            }
            "profile.discard" => {
                let (slot, _, st) = self.draft_op(slot, |d, kb| { d.discard(kb)?; Ok(true) })?;
                Ok(json!({"slot": slot, "draft": st}))
            }
            "profile.undo" => {
                let (slot, changed, st) = self.draft_op(slot, |d, kb| d.undo(kb))?;
                Ok(json!({"slot": slot, "undone": changed, "draft": st}))
            }
            "profile.redo" => {
                let (slot, changed, st) = self.draft_op(slot, |d, kb| d.redo(kb))?;
                Ok(json!({"slot": slot, "redone": changed, "draft": st}))
            }
            // ----- direct (volatile) lighting ------------------------------
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
                let pairs = p.color_map("colors")?;
                self.with_keyboard(|kb| { kb.set_keys(&pairs)?; Ok(json!({"ok": true, "keys": pairs.len()})) }).map_err(Into::into)
            }
            "rgb.set_frame" => {
                let frame = p.frame("rows")?;
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
            "rgb.save" => {
                let frame = self.with_keyboard(|kb| Ok(*kb.frame()))?;
                let (slot, _, _, st) = self.mutate(slot, true, |d, _| { d.rgb = frame; Ok(()) })?;
                Ok(json!({"ok": true, "slot": slot, "draft": st}))
            }
            // ----- stored lighting ------------------------------------------
            "rgb.profile.get" => {
                let (slot, data, _, st) = self.view(slot)?;
                let mut v = frame_json(&data.rgb);
                v["slot"] = slot.into();
                v["draft"] = serde_json::to_value(st).unwrap();
                Ok(v)
            }
            "rgb.profile.set" => {
                let rows = if p.0.get("rows").is_some() { Some(p.frame("rows")?) } else { None };
                let colors = if p.0.get("colors").is_some() { Some(p.color_map("colors")?) } else { None };
                let brightness = p.opt_u8("brightness")?;
                let (slot, data, _, st) = self.mutate(slot, persist, |d, layout| {
                    if let Some(f) = rows { d.rgb = f; }
                    if let Some(c) = &colors { d.set_key_colors(layout, c)?; }
                    if let Some(b) = brightness { d.set_brightness(b); }
                    Ok(())
                })?;
                let mut v = frame_json(&data.rgb);
                v["slot"] = slot.into();
                v["brightness"] = data.brightness().into();
                v["draft"] = serde_json::to_value(st).unwrap();
                Ok(v)
            }
            "rgb.brightness.get" => {
                let (slot, data, _, _) = self.view(slot)?;
                Ok(json!({"value": data.brightness(), "slot": slot}))
            }
            "rgb.brightness.set" => {
                let v = p.u8("value")?;
                let (slot, _, _, st) = self.mutate(slot, persist, |d, _| { d.set_brightness(v); Ok(()) })?;
                Ok(json!({"value": v, "slot": slot, "draft": st}))
            }
            "rgb.effects.get" => {
                let (slot, data, _, _) = self.view(slot)?;
                Ok(json!({"slot": slot, "brightness": data.brightness(), "layers": data.effects()}))
            }
            "rgb.effects.set" => {
                let layers: Vec<pb::EffectLayer> = serde_json::from_value(p.0.get("layers").cloned().unwrap_or(json!([]))).map_err(|e| ApiError::invalid(format!("layers: {e}")))?;
                let (slot, data, _, st) = self.mutate(slot, persist, |d, _| { d.set_effects(layers); Ok(()) })?;
                Ok(json!({"slot": slot, "layers": data.effects(), "draft": st}))
            }
            "theme.apply" => {
                let dir = match p.opt_string("dir") {
                    Some(d) => PathBuf::from(expand_home(&d)),
                    None => theme::default_theme_dir().ok_or_else(|| ApiError::invalid("no XDG state dir; pass dir"))?,
                };
                let do_persist = p.bool("persist").unwrap_or(false);
                let colors = theme::load_theme(&dir).map_err(ApiError::from)?;
                let frame = self.with_keyboard(|kb| {
                    let frame = theme::frame_from_theme(&colors, kb.layout());
                    kb.set_frame(frame)?;
                    Ok(frame)
                })?;
                if do_persist {
                    self.mutate(None, true, |d, _| { d.rgb = frame; Ok(()) })?;
                }
                Ok(json!({"ok": true, "theme_dir": dir, "colors": colors, "persisted": do_persist}))
            }
            // ----- settings ---------------------------------------------------
            "settings.get" => {
                let (slot, data, _, st) = self.view(slot)?;
                let mut v = serde_json::to_value(data.settings()).unwrap();
                v["slot"] = slot.into();
                v["draft"] = serde_json::to_value(st).unwrap();
                Ok(v)
            }
            "settings.set" => {
                let patch = p.0.clone();
                let (slot, data, _, st) = self.mutate(slot, persist, |d, _| {
                    let mut s = d.settings();
                    if let Some(v) = patch.get("actuation_mm").and_then(Value::as_f64) { s.actuation_mm = v as f32; }
                    if let Some(v) = patch.get("rapid_trigger").and_then(Value::as_bool) { s.rapid_trigger = v; }
                    if let Some(v) = patch.get("rt_press_mm").and_then(Value::as_f64) { s.rt_press_mm = v as f32; }
                    match patch.get("rt_release_mm") {
                        Some(Value::Null) => s.rt_release_mm = None,
                        Some(v) => if let Some(f) = v.as_f64() { s.rt_release_mm = Some(f as f32) },
                        None => {}
                    }
                    if let Some(v) = patch.get("continuous_rt").and_then(Value::as_bool) { s.continuous_rt = v; }
                    if let Some(v) = patch.get("tachyon").and_then(Value::as_bool) { s.tachyon = v; }
                    d.set_settings(&s);
                    Ok(())
                })?;
                let mut v = serde_json::to_value(data.settings()).unwrap();
                v["slot"] = slot.into();
                v["draft"] = serde_json::to_value(st).unwrap();
                Ok(v)
            }
            "actuation.keys.get" => {
                let (slot, data, layout, _) = self.view(slot)?;
                let m: serde_json::Map<String, Value> = data.per_key_actuation(&layout).into_iter().map(|(k, mm)| (k, json!(mm))).collect();
                Ok(json!({"slot": slot, "keys": m}))
            }
            "actuation.keys.set" => {
                let map = p.0.get("keys").and_then(Value::as_object).ok_or_else(|| ApiError::invalid("keys must be an object"))?;
                let changes: Vec<(String, Option<f32>)> = map.iter().map(|(k, v)| (k.clone(), v.as_f64().map(|f| f as f32))).collect();
                let (slot, data, layout, st) = self.mutate(slot, persist, |d, l| d.set_per_key_actuation(l, &changes))?;
                let m: serde_json::Map<String, Value> = data.per_key_actuation(&layout).into_iter().map(|(k, mm)| (k, json!(mm))).collect();
                Ok(json!({"slot": slot, "keys": m, "draft": st}))
            }
            "rapid_trigger.keys.get" => {
                let (slot, data, layout, _) = self.view(slot)?;
                Ok(json!({"slot": slot, "keys": data.rt_keys(&layout)}))
            }
            "rapid_trigger.keys.set" => {
                let map = p.0.get("keys").and_then(Value::as_object).ok_or_else(|| ApiError::invalid("keys must be an object"))?;
                let changes: Vec<(String, bool)> = map.iter().map(|(k, v)| (k.clone(), v.as_bool().unwrap_or(false))).collect();
                let (slot, data, layout, st) = self.mutate(slot, persist, |d, l| d.set_rt_keys(l, &changes))?;
                Ok(json!({"slot": slot, "keys": data.rt_keys(&layout), "draft": st}))
            }
            "settings.global.get" => self.with_keyboard(|kb| Ok(global_json(&kb.global_settings()?))).map_err(Into::into),
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
                    Ok(global_json(&kb.global_settings()?))
                }).map_err(Into::into)
            }
            "settings.device.get" => self.with_keyboard(|kb| serde_json::to_value(kb.device_settings()?).map_err(|e| Error::Other(e.to_string()))).map_err(Into::into),
            "settings.device.set" => {
                let compat = p.bool("compat_mode");
                let gamepad = p.bool("gamepad_mode");
                let polling = p.0.get("polling_hz").and_then(Value::as_u64).map(|v| v as u32);
                let controller = p.opt_string("controller");
                let any = compat.is_some() || gamepad.is_some() || polling.is_some() || controller.is_some();
                if !any {
                    return Err(ApiError::invalid("nothing to set"));
                }
                self.drafts.lock().unwrap_or_else(|p| p.into_inner()).clear();
                self.with_keyboard(|kb| {
                    kb.set_device_settings(compat, gamepad, polling, controller.as_deref())?;
                    Ok(json!({"ok": true, "restarting": true}))
                }).map_err(Into::into)
            }
            "gamepad.get" => {
                let (slot, data, layout, st) = self.view(slot)?;
                let device = self.with_keyboard(|kb| kb.device_settings()).ok();
                Ok(gamepad_json(slot, &data, &layout, &st, device))
            }
            "gamepad.set" => {
                let keys = p.0.get("keys").and_then(Value::as_object).cloned();
                let override_keys = p.bool("override");
                let keyboard_keys = p.bool("keyboard_keys");
                let (slot, data, layout, st) = self.mutate(slot, persist, |d, l| {
                    if let Some(keys) = &keys {
                        let mut changes = Vec::new();
                        for (k, v) in keys {
                            let b = match v {
                                Value::Null => None,
                                Value::String(s) => Some(gamepad::parse(s).ok_or_else(|| ApiError::invalid(format!("unknown gamepad button {s:?}; see gamepad.buttons")))?),
                                _ => return Err(ApiError::invalid("keys values must be button names or null").into()),
                            };
                            changes.push((k.clone(), b));
                        }
                        d.set_gamepad_buttons(l, &changes)?;
                    }
                    if let Some(o) = override_keys { d.set_gamepad_override(o); }
                    if let Some(k) = keyboard_keys { d.set_gamepad_keyboard_keys(k); }
                    Ok(())
                })?;
                Ok(gamepad_json(slot, &data, &layout, &st, None))
            }
            "gamepad.curve.set" => {
                let preset = p.opt_string("preset");
                let points: Option<Vec<(u32, u32)>> = p.0.get("points").and_then(Value::as_array).map(|pts| {
                    pts.iter().map(|pt| {
                        let a = pt.as_array();
                        (a.and_then(|a| a.first()).and_then(Value::as_u64).unwrap_or(0) as u32, a.and_then(|a| a.get(1)).and_then(Value::as_u64).unwrap_or(0) as u32)
                    }).collect()
                });
                let deadzone = p.0.get("deadzone").and_then(Value::as_u64).map(|v| v as u32);
                let angle_enabled = p.bool("angle_enabled");
                let angle = p.0.get("angle").and_then(Value::as_u64).map(|v| v as u32);
                let square = p.bool("square");
                let snappy = p.bool("snappy");
                let (slot, data, layout, st) = self.mutate(slot, persist, |d, _| {
                    if let Some(name) = &preset {
                        let pts = gamepad::preset(name).ok_or_else(|| ApiError::invalid(format!("unknown preset {name:?}")))?;
                        d.set_gamepad_curve(&pts)?;
                    }
                    if let Some(pts) = &points { d.set_gamepad_curve(pts)?; }
                    if let Some(dz) = deadzone { d.set_gamepad_deadzone(dz)?; }
                    let s = d.gamepad_settings_mut();
                    if let Some(a) = angle_enabled { s.angle_enabled = Some(a as u32); }
                    if let Some(a) = angle { s.angle = Some(a.min(100)); }
                    if let Some(q) = square { s.square = Some(q as u32); }
                    if let Some(n) = snappy { s.snappy = Some(n as u32); }
                    Ok(())
                })?;
                Ok(gamepad_json(slot, &data, &layout, &st, None))
            }
            "gamepad.buttons" => Ok(json!({
                "buttons": gamepad::BUTTONS.iter().map(|(n, l, c)| json!({"name": n, "label": l, "code": c})).collect::<Vec<_>>(),
                "presets": gamepad::CURVE_PRESETS.iter().map(|(n, p)| json!({"name": n, "points": p})).collect::<Vec<_>>(),
            })),
            "settings.reset" => {
                if !p.bool("confirm").unwrap_or(false) {
                    return Err(ApiError::invalid("resetting the keyboard is irreversible; pass confirm=true"));
                }
                self.drafts.lock().unwrap_or_else(|p| p.into_inner()).clear();
                self.with_keyboard(|kb| { kb.cmd(Command::ResetSettings, [0; 4])?; Ok(json!({"ok": true})) }).map_err(Into::into)
            }
            // ----- key map ----------------------------------------------------
            "keymap.get" => {
                let layer = p.opt_u8("layer")?.unwrap_or(0);
                let (slot, data, layout, st) = self.view(slot)?;
                let stored = data.mapping.get(layer as usize).map(|m| m.is_some()).unwrap_or(false);
                Ok(mapping_json(&layout, slot, layer, &data.mapping_layer(layer, &layout), stored, &st))
            }
            "keymap.set_key" => {
                let key = p.string("key")?;
                let code = p.keycode("code")?;
                let layer = p.opt_u8("layer")?.unwrap_or(0);
                let (slot, data, layout, st) = self.mutate(slot, persist, |d, l| d.set_keys(l, layer, &[(key, code)]))?;
                Ok(mapping_json(&layout, slot, layer, &data.mapping_layer(layer, &layout), true, &st))
            }
            "keymap.set" => {
                let map = p.0.get("keys").and_then(Value::as_object).ok_or_else(|| ApiError::invalid("keys must be an object"))?;
                let layer = p.opt_u8("layer")?.unwrap_or(0);
                let mut pairs = Vec::new();
                for (k, v) in map {
                    let s = v.as_str().ok_or_else(|| ApiError::invalid(format!("code for {k} must be a string")))?;
                    pairs.push((k.clone(), keycode::parse(s).ok_or_else(|| ApiError::invalid(format!("unknown key code {s:?}")))?));
                }
                let (slot, data, layout, st) = self.mutate(slot, persist, |d, l| d.set_keys(l, layer, &pairs))?;
                Ok(mapping_json(&layout, slot, layer, &data.mapping_layer(layer, &layout), true, &st))
            }
            "keymap.set_layer" => {
                let layer = p.opt_u8("layer")?.unwrap_or(0);
                let preset = p.opt_string("preset");
                let rows_param = p.0.get("rows").cloned();
                let (slot, data, layout, st) = self.mutate(slot, persist, |d, l| {
                    let rows: Option<Rows> = if let Some(name) = &preset {
                        Some(wooting_proto::layout::preset_mapping(name, l).ok_or_else(|| Error::Other(format!("unknown preset {name:?}")))?)
                    } else {
                        match rows_param {
                            Some(Value::Null) | None => None,
                            Some(v) => Some(parse_rows(&v)?),
                        }
                    };
                    d.set_layer(layer, rows);
                    Ok(())
                })?;
                let stored = data.mapping.get(layer as usize).map(|m| m.is_some()).unwrap_or(false);
                Ok(mapping_json(&layout, slot, layer, &data.mapping_layer(layer, &layout), stored, &st))
            }
            "keymap.reset" => {
                let layer = p.opt_u8("layer")?.unwrap_or(0);
                let (slot, data, layout, st) = self.mutate(slot, persist, |d, _| { d.set_layer(layer, None); Ok(()) })?;
                Ok(mapping_json(&layout, slot, layer, &data.mapping_layer(layer, &layout), false, &st))
            }
            "keycodes.list" => Ok(json!({"codes": keycode::KEYCODES.iter().map(|(c, n)| json!({"code": c, "name": n, "label": keycode::label(*c)})).collect::<Vec<_>>()})),
            // ----- advanced keys ------------------------------------------------
            "advanced_keys.get" => {
                let (slot, data, layout, st) = self.view(slot)?;
                Ok(json!({"slot": slot, "entries": data.advanced_keys(&layout), "draft": st}))
            }
            "advanced_keys.set" | "advanced_keys.set_mod_tap" => {
                let key = p.string("key")?;
                let kind = if method == "advanced_keys.set_mod_tap" { "mod_tap".to_string() } else { p.string("kind")? };
                let (slot, data, layout, st) = self.mutate(slot, persist, |d, l| {
                    let entry = match kind.as_str() {
                        "mod_tap" => Keyboard::akc_mod_tap(p.keycode("tap")?, p.keycode("hold")?),
                        "toggle" => Keyboard::akc_toggle(p.keycode("code")?),
                        "rappy_snappy" => pb::advanced_keys::Entry {
                            rappy_snappy: Some(pb::advanced_keys::RappySnappy { other_key: Some(d.matrix_of(l, &p.string("other_key")?)?), p2: Some(0) }),
                            ..Default::default()
                        },
                        "socd" => pb::advanced_keys::Entry {
                            socd: Some(pb::advanced_keys::Socd { other_key: Some(d.matrix_of(l, &p.string("other_key")?)?), p2: Some(0), mode: Some(p.0.get("mode").and_then(Value::as_u64).unwrap_or(1) as u32) }),
                            ..Default::default()
                        },
                        "dks" => {
                            let names = p.0.get("codes").and_then(Value::as_array).ok_or_else(|| ApiError::invalid("dks needs codes"))?;
                            let mut codes = [0xffu8; 4];
                            for (i, n) in names.iter().take(4).enumerate() {
                                let s = n.as_str().unwrap_or("none");
                                codes[i] = keycode::parse(s).ok_or_else(|| ApiError::invalid(format!("unknown key code {s:?}")))?;
                            }
                            let masks = p.0.get("masks").and_then(Value::as_array).map(|m| {
                                let mut out = [0u8; 4];
                                for (i, v) in m.iter().take(4).enumerate() { out[i] = v.as_u64().unwrap_or(0) as u8; }
                                out
                            });
                            Keyboard::akc_dks(codes, masks)
                        }
                        other => return Err(ApiError::invalid(format!("unknown kind {other:?}")).into()),
                    };
                    d.set_advanced_key(l, &key, entry)
                })?;
                Ok(json!({"slot": slot, "entries": data.advanced_keys(&layout), "draft": st}))
            }
            "advanced_keys.remove" => {
                let key = p.string("key")?;
                let (slot, data, layout, st) = self.mutate(slot, persist, |d, l| d.remove_advanced_key(l, &key))?;
                Ok(json!({"slot": slot, "entries": data.advanced_keys(&layout), "draft": st}))
            }
            // ----- misc ------------------------------------------------------------
            "backup.create" => self.with_keyboard(|kb| Ok(json!({"path": backup::save(kb)?}))).map_err(Into::into),
            "raw.command" => {
                let name = p.string("command")?;
                let cmd = Command::parse(&name).ok_or_else(|| ApiError::invalid(format!("unknown command {name:?}")))?;
                if cmd.is_dangerous() && !p.bool("confirm").unwrap_or(false) {
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
            m if m.starts_with("links.") => Err(ApiError { code: -32601, message: "linked profiles need the wootd daemon".into() }),
            other => Err(ApiError::method_not_found(other)),
        }
    }
}

fn gamepad_json(
    slot: u8,
    d: &ProfileData,
    layout: &Layout,
    st: &DraftStatus,
    device: Option<crate::keyboard::DeviceSettings>,
) -> Value {
    let keys: serde_json::Map<String, Value> = d
        .gamepad_buttons(layout)
        .into_iter()
        .map(|(k, b)| (k, Value::String(b)))
        .collect();
    let s = d.gamepad.settings.unwrap_or_default();
    let curve = d.gamepad_curve();
    json!({
        "slot": slot,
        "keys": keys,
        "override": d.gamepad_override(),
        "keyboard_keys": d.gamepad_keyboard_keys(),
        "curve": curve,
        "preset": gamepad::preset_name(&curve),
        "deadzone": curve.first().map(|p| p.1).unwrap_or(0),
        "angle_enabled": s.angle_enabled.unwrap_or(0) != 0,
        "angle": s.angle.unwrap_or(50),
        "square": s.square.unwrap_or(0) != 0,
        "snappy": s.snappy.unwrap_or(1) != 0,
        "device": device,
        "draft": st,
    })
}

fn global_json(g: &pb::GlobalSettings) -> Value {
    let ps = g.power_saving.unwrap_or_default();
    json!({
        "dim_after_minutes": ps.dim_after_minutes.unwrap_or(0),
        "off_after_minutes": ps.off_after_minutes.unwrap_or(0),
        "numlock_inverted": g.numlock_inverted.unwrap_or(0) != 0,
        "debounce_filter": g.debounce_filter.unwrap_or(0) != 0,
    })
}

fn parse_rows(v: &Value) -> Result<Rows> {
    let mut rows = [[keycode::NONE; COLS]; ROWS];
    for (r, row) in v.as_array().into_iter().flatten().take(ROWS).enumerate() {
        for (c, cell) in row.as_array().into_iter().flatten().take(COLS).enumerate() {
            rows[r][c] = match cell {
                Value::Number(n) => n.as_u64().unwrap_or(255) as u8,
                Value::String(s) => keycode::parse(s)
                    .ok_or_else(|| Error::Other(format!("unknown key code {s:?}")))?,
                _ => keycode::NONE,
            };
        }
    }
    Ok(rows)
}

fn mapping_json(
    layout: &Layout,
    slot: u8,
    layer: u8,
    rows: &Rows,
    stored: bool,
    st: &DraftStatus,
) -> Value {
    let keys: serde_json::Map<String, Value> = layout
        .keys
        .iter()
        .map(|k| {
            let c = k.cells[0];
            let code = rows[c[0] as usize][c[1] as usize];
            (
                k.id.clone(),
                json!({"code": code, "name": keycode::name(code), "label": keycode::label(code)}),
            )
        })
        .collect();
    json!({"slot": slot, "layer": layer, "stored": stored, "keys": keys, "rows": rows.iter().map(|r| r.to_vec()).collect::<Vec<_>>(), "draft": st})
}

fn frame_json(f: &Frame) -> Value {
    json!({"rows": f.0.iter().map(|r| r.iter().map(|c| c.to_hex()).collect::<Vec<_>>()).collect::<Vec<_>>()})
}

fn expand_home(p: &str) -> String {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()
            .map(|h| h.join(rest).to_string_lossy().into_owned())
            .unwrap_or_else(|| p.to_string()),
        None => p.to_string(),
    }
}

struct Params(Value);

impl Params {
    fn string(&self, k: &str) -> std::result::Result<String, ApiError> {
        self.opt_string(k)
            .ok_or_else(|| ApiError::invalid(format!("missing string param {k:?}")))
    }
    fn opt_string(&self, k: &str) -> Option<String> {
        self.0.get(k).and_then(Value::as_str).map(str::to_owned)
    }
    fn bool(&self, k: &str) -> Option<bool> {
        self.0.get(k).and_then(Value::as_bool)
    }
    fn opt_u8(&self, k: &str) -> std::result::Result<Option<u8>, ApiError> {
        match self.0.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.u8(k).map(Some),
        }
    }
    fn u8(&self, k: &str) -> std::result::Result<u8, ApiError> {
        self.0
            .get(k)
            .and_then(Value::as_u64)
            .filter(|v| *v < 256)
            .map(|v| v as u8)
            .ok_or_else(|| ApiError::invalid(format!("param {k:?} must be an integer 0-255")))
    }
    fn keycode(&self, k: &str) -> std::result::Result<u8, ApiError> {
        let s = self.string(k)?;
        keycode::parse(&s).ok_or_else(|| {
            ApiError::invalid(format!("{k}: unknown key code {s:?}; see keycodes.list"))
        })
    }
    fn color(&self, k: &str) -> std::result::Result<Rgb, ApiError> {
        let s = self.string(k)?;
        Rgb::parse_hex(&s)
            .ok_or_else(|| ApiError::invalid(format!("{k}: bad colour {s:?}, expected #rrggbb")))
    }
    fn opt_color(&self, k: &str) -> std::result::Result<Option<Rgb>, ApiError> {
        match self.opt_string(k) {
            Some(_) => self.color(k).map(Some),
            None => Ok(None),
        }
    }
    fn color_map(&self, k: &str) -> std::result::Result<Vec<(String, Rgb)>, ApiError> {
        let map = self
            .0
            .get(k)
            .and_then(Value::as_object)
            .ok_or_else(|| ApiError::invalid(format!("{k} must be an object")))?;
        let mut pairs = Vec::new();
        for (key, v) in map {
            let s = v
                .as_str()
                .ok_or_else(|| ApiError::invalid(format!("colour for {key} must be a string")))?;
            pairs.push((
                key.clone(),
                Rgb::parse_hex(s).ok_or_else(|| ApiError::invalid(format!("bad colour {s:?}")))?,
            ));
        }
        Ok(pairs)
    }
    fn frame(&self, k: &str) -> std::result::Result<Frame, ApiError> {
        let rows = self
            .0
            .get(k)
            .and_then(Value::as_array)
            .ok_or_else(|| ApiError::invalid(format!("{k} must be an array")))?;
        let mut frame = Frame::default();
        for (r, row) in rows.iter().take(ROWS).enumerate() {
            for (c, v) in row.as_array().into_iter().flatten().take(COLS).enumerate() {
                if let Some(s) = v.as_str() {
                    frame.set(
                        r,
                        c,
                        Rgb::parse_hex(s)
                            .ok_or_else(|| ApiError::invalid(format!("bad colour {s:?}")))?,
                    );
                }
            }
        }
        Ok(frame)
    }
}

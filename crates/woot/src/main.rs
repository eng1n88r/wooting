//! `woot`: thin CLI over the shared API. Talks to a running `wootd` when its
//! socket is up, otherwise opens the keyboard directly.

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use serde_json::{json, Value};
use std::{thread, time::Duration};
use wooting_core::client::Backend;
use wooting_proto::{Command, Rgb};

#[derive(Parser)]
#[command(name = "woot", version, about = "Control Wooting keyboards from the terminal")]
struct Cli {
    /// Select a keyboard by hidraw path or serial number (direct mode only).
    #[arg(long, global = true)]
    device: Option<String>,
    /// Print raw JSON results.
    #[arg(long, global = true)]
    json: bool,
    /// Never use the daemon; open the keyboard directly.
    #[arg(long, global = true)]
    direct: bool,
    /// Edit the live draft only (no flash write); commit later with `woot profile save`.
    #[arg(long, global = true)]
    draft: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List attached Wooting keyboards.
    Devices,
    /// Model, firmware, serial, active profile.
    Info,
    /// Send Ping and report the round trip.
    Ping,
    /// Keyboard profiles.
    Profile(ProfileArgs),
    /// Lighting.
    Rgb(RgbArgs),
    /// Colour the keyboard from an Omarchy theme.
    Theme(ThemeArgs),
    /// Dump every readable setting to a JSON backup file.
    Backup,
    /// Key mapping.
    Map(MapArgs),
    /// Actuation point, rapid trigger, Tachyon.
    Actuation(ActuationArgs),
    /// Advanced keys (mod-tap).
    Akc(AkcArgs),
    /// Device-wide settings (RGB power saving, Num Lock, debounce, 6KRO, gamepad).
    Settings {
        /// Minutes until lighting dims (0 = never).
        #[arg(long)]
        dim: Option<u32>,
        /// Minutes until lighting turns off (0 = never).
        #[arg(long)]
        off: Option<u32>,
        #[arg(long)]
        numlock_inverted: Option<bool>,
        #[arg(long)]
        debounce: Option<bool>,
        /// Compatibility mode (6KRO); restarts the keyboard.
        #[arg(long)]
        compat: Option<bool>,
        /// Gamepad mode; restarts the keyboard with a different USB identity.
        #[arg(long)]
        gamepad: Option<bool>,
        /// Reset ALL keyboard settings to factory (needs --confirm).
        #[arg(long)]
        reset: bool,
        #[arg(long)]
        confirm: bool,
    },
    /// Call any API method with a JSON params object.
    Call { method: String, params: Option<String> },
    /// List API methods.
    Methods,
    /// Raw protocol access (for protocol work).
    Raw(RawArgs),
}

#[derive(Args)]
struct ProfileArgs {
    #[command(subcommand)]
    cmd: ProfileCmd,
}

#[derive(Subcommand)]
enum ProfileCmd {
    /// List profiles.
    List,
    /// Activate profile slot N (0-based) or by name.
    Activate { slot: String },
    /// Rename a profile slot and/or set its badge colour.
    Rename {
        slot: u8,
        name: Option<String>,
        #[arg(long)]
        color: Option<Rgb>,
    },
    /// Delete a profile slot (irreversible).
    Delete {
        slot: u8,
        #[arg(long)]
        confirm: bool,
    },
    /// Create a profile in the first free slot, copied from another slot.
    Create {
        name: String,
        #[arg(long, default_value_t = 0)]
        from: u8,
        #[arg(long)]
        color: Option<Rgb>,
    },
    /// Swap two slots (slot 0 is the default profile).
    Swap { a: u8, b: u8 },
    /// Draft status of a slot (dirty / undo / redo).
    Status { slot: Option<u8> },
    /// Write the draft to flash.
    Save { slot: Option<u8> },
    /// Drop pending edits and reload from flash.
    Discard { slot: Option<u8> },
    /// Undo the last draft edit.
    Undo { slot: Option<u8> },
    /// Redo the last undone edit.
    Redo { slot: Option<u8> },
    /// Export a slot to a JSON file (or stdout).
    Export { slot: Option<u8>, file: Option<String> },
    /// Import a JSON file into a slot.
    Import {
        slot: u8,
        file: String,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Args)]
struct MapArgs {
    #[command(subcommand)]
    cmd: MapCmd,
}

#[derive(Subcommand)]
enum MapCmd {
    /// Show a layer's mapping (0 = main, 1/2 = Fn layers).
    Show {
        #[arg(long, default_value_t = 0)]
        layer: u8,
        #[arg(long)]
        slot: Option<u8>,
    },
    /// Remap keys: `woot map set capslock=esc esc=grave`.
    Set {
        assignments: Vec<String>,
        #[arg(long, default_value_t = 0)]
        layer: u8,
    },
    /// Replace a layer with a preset: default|function|dvorak|colemak|workman|abc.
    Preset {
        name: String,
        #[arg(long, default_value_t = 0)]
        layer: u8,
    },
    /// Restore the factory mapping of a layer.
    Reset {
        #[arg(long, default_value_t = 0)]
        layer: u8,
    },
    /// List key code names.
    Codes,
}

#[derive(Args)]
struct AkcArgs {
    #[command(subcommand)]
    cmd: AkcCmd,
}

#[derive(Subcommand)]
enum AkcCmd {
    /// List advanced keys of a profile slot (default: active).
    List {
        #[arg(long)]
        slot: Option<u8>,
    },
    /// Make a key a mod-tap: `woot akc modtap capslock --hold lctrl --tap esc`.
    Modtap {
        key: String,
        #[arg(long)]
        hold: String,
        #[arg(long)]
        tap: String,
    },
    /// Toggle key: tapping locks `code` on/off, holding acts normally.
    Toggle { key: String, code: String },
    /// Rappy Snappy between two keys (whichever is pressed deeper wins).
    Rappy { key: String, other: String },
    /// SOCD (Snappy Tappy) between two keys.
    Socd {
        key: String,
        other: String,
        #[arg(long, default_value_t = 1)]
        mode: u32,
    },
    /// DKS: up to four bindings for four travel phases.
    Dks {
        key: String,
        #[arg(num_args = 1..=4)]
        codes: Vec<String>,
    },
    /// Remove the advanced-key entry from a key.
    Remove { key: String },
}

#[derive(Args)]
struct ActuationArgs {
    #[command(subcommand)]
    cmd: ActuationCmd,
}

#[derive(Subcommand)]
enum ActuationCmd {
    /// Show settings of a profile slot (default: active).
    Show {
        #[arg(long)]
        slot: Option<u8>,
    },
    /// Change settings for all keys of the active profile.
    Set {
        /// Actuation point in mm (0.1-4.0).
        #[arg(long)]
        point: Option<f32>,
        /// Rapid trigger on/off.
        #[arg(long)]
        rapid_trigger: Option<bool>,
        /// Rapid trigger press sensitivity in mm.
        #[arg(long)]
        press: Option<f32>,
        /// Rapid trigger release sensitivity in mm (omit = same as press).
        #[arg(long)]
        release: Option<f32>,
        /// Drop the separate release sensitivity.
        #[arg(long)]
        no_split: bool,
        /// Continuous rapid trigger on/off.
        #[arg(long)]
        continuous: Option<bool>,
        /// Tachyon mode on/off.
        #[arg(long)]
        tachyon: Option<bool>,
    },
    /// Per-key actuation: `woot actuation keys a=2.5 s=1.0 d=-` (- removes).
    Keys { assignments: Vec<String> },
    /// Per-key rapid trigger: `woot actuation rt w=on a=off`.
    Rt { assignments: Vec<String> },
}

#[derive(Args)]
struct RgbArgs {
    #[command(subcommand)]
    cmd: RgbCmd,
}

#[derive(Subcommand)]
enum RgbCmd {
    /// Light every key with one colour.
    Fill { color: Rgb },
    /// Set one key by id (see `woot rgb keys`).
    Key { key: String, color: Rgb },
    /// Set several keys: `woot rgb keys esc=#ff0000 space=#00ff00`.
    Keys { assignments: Vec<String> },
    /// Paint the keyboard with an image.
    Image {
        path: String,
        #[arg(long, default_value = "cover")]
        mode: String,
        #[arg(long)]
        gamma: Option<f32>,
        #[arg(long)]
        brightness: Option<f32>,
    },
    /// Show the current direct-mode frame.
    Frame,
    /// Show the colours stored in an RGB profile slot (default: active).
    Stored {
        #[arg(long)]
        slot: Option<u8>,
    },
    /// Persist the current frame into an RGB profile slot (flash; default: active).
    Save {
        #[arg(long)]
        slot: Option<u8>,
    },
    /// Get or set stored brightness (0-255).
    Brightness {
        value: Option<u8>,
        #[arg(long)]
        slot: Option<u8>,
    },
    /// Hand the LEDs back to the keyboard.
    Release,
    /// List key ids and positions.
    List,
    /// Light keys one at a time to verify the layout.
    Identify {
        #[arg(long, default_value_t = 500)]
        delay_ms: u64,
    },
}

#[derive(Args)]
struct ThemeArgs {
    #[command(subcommand)]
    cmd: ThemeCmd,
}

#[derive(Subcommand)]
enum ThemeCmd {
    /// Apply a theme directory (default: current Omarchy theme).
    Apply {
        dir: Option<String>,
        /// Also save it into the keyboard so it survives unplugging.
        #[arg(long)]
        persist: bool,
    },
}

#[derive(Args)]
struct RawArgs {
    #[command(subcommand)]
    cmd: RawCmd,
}

#[derive(Subcommand)]
enum RawCmd {
    /// Send a feature command by name or id with up to four parameters.
    Cmd {
        command: String,
        #[arg(num_args = 0..=4)]
        params: Vec<u8>,
        /// Required for commands that reset or reflash the keyboard.
        #[arg(long)]
        confirm: bool,
    },
    /// Write a raw output report given as hex (first byte = report id). Direct mode only.
    Report { hex: String },
    /// Read input reports for N seconds. Direct mode only.
    Listen {
        #[arg(default_value_t = 5)]
        seconds: u64,
    },
    /// List all known command ids.
    Commands,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let json = cli.json;

    let mut backend = if cli.direct {
        Backend::Local(wooting_core::api::Api::new(cli.device.clone()))
    } else {
        Backend::auto(cli.device.clone())
    };
    let draft = cli.draft;
    let mut call = |method: &str, mut params: Value| -> Result<Value> {
        if draft {
            if let Value::Object(m) = &mut params {
                m.entry("persist").or_insert(Value::Bool(false));
            }
        }
        backend
            .call(method, params)
            .map_err(|e| anyhow::anyhow!("{method}: {}", e.message))
    };

    let out: Value = match cli.cmd {
        Cmd::Devices => {
            let devs = wooting_hid::enumerate()?;
            if !json {
                for d in &devs {
                    println!(
                        "{:<22} {:04x}:{:04x} {:<13} {:?} serial={}",
                        d.model_name(),
                        d.vid,
                        d.pid,
                        d.path,
                        d.protocol,
                        d.serial.as_deref().unwrap_or("-")
                    );
                }
                return Ok(());
            }
            serde_json::to_value(devs)?
        }
        Cmd::Info => call("device.info", json!({}))?,
        Cmd::Ping => {
            let start = std::time::Instant::now();
            let r = call("raw.command", json!({"command": "ping"}))?;
            if !json {
                println!("pong in {:?} (status {})", start.elapsed(), r["status_name"]);
                return Ok(());
            }
            r
        }
        Cmd::Profile(ProfileArgs { cmd }) => match cmd {
            ProfileCmd::List => {
                let v = call("profile.list", json!({}))?;
                if !json {
                    for p in v.as_array().into_iter().flatten() {
                        println!(
                            "{} {}  {:<24} {}",
                            if p["active"].as_bool().unwrap_or(false) { "*" } else { " " },
                            p["slot"],
                            p["name"].as_str().unwrap_or(""),
                            p["icon"].as_str().unwrap_or("")
                        );
                    }
                    return Ok(());
                }
                v
            }
            ProfileCmd::Activate { slot } => {
                let n = match slot.parse::<u8>() {
                    Ok(n) => n,
                    Err(_) => {
                        let list = call("profile.list", json!({}))?;
                        list.as_array()
                            .into_iter()
                            .flatten()
                            .find(|p| p["name"].as_str().map(|n| n.eq_ignore_ascii_case(&slot)).unwrap_or(false))
                            .and_then(|p| p["slot"].as_u64())
                            .with_context(|| format!("no profile named {slot:?}"))? as u8
                    }
                };
                call("profile.activate", json!({"slot": n}))?
            }
            ProfileCmd::Rename { slot, name, color } => {
                let mut p = json!({"slot": slot});
                if let Some(n) = name { p["name"] = n.into(); }
                if let Some(c) = color { p["color"] = c.to_hex().into(); }
                call("profile.rename", p)?
            }
            ProfileCmd::Delete { slot, confirm } => call("profile.delete", json!({"slot": slot, "confirm": confirm}))?,
            ProfileCmd::Create { name, from, color } => {
                let mut p = json!({"name": name, "from_slot": from});
                if let Some(c) = color { p["color"] = c.to_hex().into(); }
                call("profile.create", p)?
            }
            ProfileCmd::Swap { a, b } => call("profile.swap", json!({"a": a, "b": b}))?,
            ProfileCmd::Status { slot } => call("profile.status", json!({"slot": slot}))?,
            ProfileCmd::Save { slot } => call("profile.save", json!({"slot": slot}))?,
            ProfileCmd::Discard { slot } => call("profile.discard", json!({"slot": slot}))?,
            ProfileCmd::Undo { slot } => call("profile.undo", json!({"slot": slot}))?,
            ProfileCmd::Redo { slot } => call("profile.redo", json!({"slot": slot}))?,
            ProfileCmd::Export { slot, file } => {
                let v = call("profile.export", json!({"slot": slot}))?;
                match file {
                    Some(f) => {
                        std::fs::write(&f, serde_json::to_string_pretty(&v)?)?;
                        json!({"written": f})
                    }
                    None => {
                        println!("{}", serde_json::to_string_pretty(&v)?);
                        return Ok(());
                    }
                }
            }
            ProfileCmd::Import { slot, file, name } => {
                let text = std::fs::read_to_string(&file).with_context(|| format!("reading {file}"))?;
                let v: Value = serde_json::from_str(&text)?;
                let data = v.get("data").cloned().unwrap_or(v);
                let mut p = json!({"slot": slot, "data": data});
                if let Some(n) = name { p["name"] = n.into(); }
                call("profile.import", p)?
            }
        },
        Cmd::Rgb(RgbArgs { cmd }) => match cmd {
            RgbCmd::Fill { color } => call("rgb.fill", json!({"color": color.to_hex()}))?,
            RgbCmd::Key { key, color } => call("rgb.set_key", json!({"key": key, "color": color.to_hex()}))?,
            RgbCmd::Keys { assignments } => {
                let mut colors = serde_json::Map::new();
                for a in assignments {
                    let (k, v) = a.split_once('=').with_context(|| format!("expected key=#rrggbb, got {a:?}"))?;
                    colors.insert(k.to_string(), Value::String(v.to_string()));
                }
                call("rgb.set_keys", json!({"colors": colors}))?
            }
            RgbCmd::Image { path, mode, gamma, brightness } => {
                let path = std::fs::canonicalize(&path).with_context(|| format!("image {path:?}"))?;
                let mut p = json!({"path": path, "mode": mode});
                if let Some(g) = gamma { p["gamma"] = g.into(); }
                if let Some(b) = brightness { p["brightness"] = b.into(); }
                call("rgb.from_image", p)?
            }
            RgbCmd::Frame => call("rgb.get_frame", json!({}))?,
            RgbCmd::Stored { slot } => call("rgb.profile.get", json!({"slot": slot}))?,
            RgbCmd::Save { slot } => call("rgb.save", json!({"slot": slot}))?,
            RgbCmd::Brightness { value, slot } => match value {
                Some(v) => call("rgb.brightness.set", json!({"value": v, "slot": slot}))?,
                None => call("rgb.brightness.get", json!({"slot": slot}))?,
            },
            RgbCmd::Release => call("rgb.release", json!({}))?,
            RgbCmd::List => {
                let v = call("device.keys", json!({}))?;
                if !json {
                    for k in v["keys"].as_array().into_iter().flatten() {
                        println!("{:<10} {:<10} cells={}", k["id"].as_str().unwrap_or(""), k["label"].as_str().unwrap_or(""), k["cells"]);
                    }
                    return Ok(());
                }
                v
            }
            RgbCmd::Identify { delay_ms } => {
                let keys = call("device.keys", json!({}))?;
                call("rgb.fill", json!({"color": "#000000"}))?;
                for k in keys["keys"].as_array().into_iter().flatten() {
                    let id = k["id"].as_str().unwrap_or("");
                    println!("{id}");
                    call("rgb.set_keys", json!({"colors": {id: "#ffffff"}}))?;
                    thread::sleep(Duration::from_millis(delay_ms));
                    call("rgb.set_keys", json!({"colors": {id: "#000000"}}))?;
                }
                call("rgb.release", json!({}))?
            }
        },
        Cmd::Theme(ThemeArgs { cmd }) => match cmd {
            ThemeCmd::Apply { dir, persist } => {
                let mut p = json!({"persist": persist});
                if let Some(d) = dir { p["dir"] = d.into(); }
                call("theme.apply", p)?
            }
        },
        Cmd::Backup => call("backup.create", json!({}))?,
        Cmd::Map(MapArgs { cmd }) => match cmd {
            MapCmd::Show { layer, slot } => {
                let v = call("keymap.get", json!({"layer": layer, "slot": slot}))?;
                if !json {
                    print_mapping(&v);
                    return Ok(());
                }
                v
            }
            MapCmd::Set { assignments, layer } => {
                let mut keys = serde_json::Map::new();
                for a in assignments {
                    let (k, v) = a.split_once('=').with_context(|| format!("expected key=code, got {a:?}"))?;
                    keys.insert(k.to_string(), Value::String(v.to_string()));
                }
                let v = call("keymap.set", json!({"keys": keys, "layer": layer}))?;
                if !json {
                    print_mapping(&v);
                    return Ok(());
                }
                v
            }
            MapCmd::Preset { name, layer } => {
                let v = call("keymap.set_layer", json!({"layer": layer, "preset": name}))?;
                if !json {
                    print_mapping(&v);
                    return Ok(());
                }
                v
            }
            MapCmd::Reset { layer } => call("keymap.reset", json!({"layer": layer}))?,
            MapCmd::Codes => {
                let v = call("keycodes.list", json!({}))?;
                if !json {
                    for c in v["codes"].as_array().into_iter().flatten() {
                        println!("0x{:02x}  {}", c["code"].as_u64().unwrap_or(0), c["name"].as_str().unwrap_or(""));
                    }
                    return Ok(());
                }
                v
            }
        },
        Cmd::Akc(AkcArgs { cmd }) => {
            let v = match cmd {
                AkcCmd::List { slot } => call("advanced_keys.get", json!({"slot": slot}))?,
                AkcCmd::Modtap { key, hold, tap } => call("advanced_keys.set", json!({"key": key, "kind": "mod_tap", "hold": hold, "tap": tap}))?,
                AkcCmd::Toggle { key, code } => call("advanced_keys.set", json!({"key": key, "kind": "toggle", "code": code}))?,
                AkcCmd::Rappy { key, other } => call("advanced_keys.set", json!({"key": key, "kind": "rappy_snappy", "other_key": other}))?,
                AkcCmd::Socd { key, other, mode } => call("advanced_keys.set", json!({"key": key, "kind": "socd", "other_key": other, "mode": mode}))?,
                AkcCmd::Dks { key, codes } => call("advanced_keys.set", json!({"key": key, "kind": "dks", "codes": codes}))?,
                AkcCmd::Remove { key } => call("advanced_keys.remove", json!({"key": key}))?,
            };
            if !json {
                for e in v["entries"].as_array().into_iter().flatten() {
                    let detail = match e["kind"].as_str().unwrap_or("?") {
                        "mod_tap" => format!("hold={} tap={}", e["hold"].as_str().unwrap_or("-"), e["tap"].as_str().unwrap_or("-")),
                        "toggle" => format!("toggles {}", e["tap"].as_str().unwrap_or("-")),
                        "rappy_snappy" => format!("with {}", e["other_key"].as_str().unwrap_or("-")),
                        "socd" => format!("with {} mode {}", e["other_key"].as_str().unwrap_or("-"), e["mode"]),
                        "dks" => format!("{}", e["dks_codes"]),
                        _ => String::new(),
                    };
                    println!("{:<10} {:<13} {}", e["key"].as_str().unwrap_or("?"), e["kind"].as_str().unwrap_or("?"), detail);
                }
                return Ok(());
            }
            v
        }
        Cmd::Actuation(ActuationArgs { cmd }) => match cmd {
            ActuationCmd::Show { slot } => call("settings.get", json!({"slot": slot}))?,
            ActuationCmd::Set { point, rapid_trigger, press, release, no_split, continuous, tachyon } => {
                let mut p = json!({});
                if let Some(v) = point { p["actuation_mm"] = v.into(); }
                if let Some(v) = rapid_trigger { p["rapid_trigger"] = v.into(); }
                if let Some(v) = press { p["rt_press_mm"] = v.into(); }
                if let Some(v) = release { p["rt_release_mm"] = v.into(); }
                if no_split { p["rt_release_mm"] = Value::Null; }
                if let Some(v) = continuous { p["continuous_rt"] = v.into(); }
                if let Some(v) = tachyon { p["tachyon"] = v.into(); }
                call("settings.set", p)?
            }
            ActuationCmd::Keys { assignments } => {
                if assignments.is_empty() {
                    call("actuation.keys.get", json!({}))?
                } else {
                    let mut keys = serde_json::Map::new();
                    for a in assignments {
                        let (k, v) = a.split_once('=').with_context(|| format!("expected key=mm, got {a:?}"))?;
                        keys.insert(k.to_string(), if v == "-" { Value::Null } else { json!(v.parse::<f64>().context("mm must be a number")?) });
                    }
                    call("actuation.keys.set", json!({"keys": keys}))?
                }
            }
            ActuationCmd::Rt { assignments } => {
                if assignments.is_empty() {
                    call("rapid_trigger.keys.get", json!({}))?
                } else {
                    let mut keys = serde_json::Map::new();
                    for a in assignments {
                        let (k, v) = a.split_once('=').with_context(|| format!("expected key=on|off, got {a:?}"))?;
                        keys.insert(k.to_string(), Value::Bool(matches!(v, "on" | "true" | "1")));
                    }
                    call("rapid_trigger.keys.set", json!({"keys": keys}))?
                }
            }
        },
        Cmd::Settings { dim, off, numlock_inverted, debounce, compat, gamepad, reset, confirm } => {
            if reset {
                call("settings.reset", json!({"confirm": confirm}))?
            } else if compat.is_some() || gamepad.is_some() {
                let mut p = json!({});
                if let Some(c) = compat { p["compat_mode"] = c.into(); }
                if let Some(g) = gamepad { p["gamepad_mode"] = g.into(); }
                call("settings.device.set", p)?
            } else if dim.is_none() && off.is_none() && numlock_inverted.is_none() && debounce.is_none() {
                let mut g = call("settings.global.get", json!({}))?;
                let d = call("settings.device.get", json!({}))?;
                g["compat_mode"] = d["compat_mode"].clone();
                g["gamepad_mode"] = d["gamepad_mode"].clone();
                g
            } else {
                let mut p = json!({});
                if let Some(d) = dim { p["dim_after_minutes"] = d.into(); }
                if let Some(o) = off { p["off_after_minutes"] = o.into(); }
                if let Some(n) = numlock_inverted { p["numlock_inverted"] = n.into(); }
                if let Some(b) = debounce { p["debounce_filter"] = b.into(); }
                call("settings.global.set", p)?
            }
        }
        Cmd::Call { method, params } => {
            let p: Value = match params {
                Some(s) => serde_json::from_str(&s).context("params must be a JSON object")?,
                None => json!({}),
            };
            call(&method, p)?
        }
        Cmd::Methods => {
            if !json {
                for m in wooting_core::api::methods() {
                    println!("{:<22} {}", m.name, m.description);
                }
                return Ok(());
            }
            serde_json::to_value(wooting_core::api::methods())?
        }
        Cmd::Raw(RawArgs { cmd }) => match cmd {
            RawCmd::Commands => {
                for c in Command::ALL {
                    println!("{:>2}  {}", *c as u8, c.name());
                }
                return Ok(());
            }
            RawCmd::Cmd { command, params, confirm } => {
                let r = call("raw.command", json!({"command": command, "params": params, "confirm": confirm}))?;
                if !json {
                    println!("{} status={} ({}) data={}", r["command"].as_str().unwrap_or(""), r["status"], r["status_name"].as_str().unwrap_or(""),
                        r["data"].as_array().into_iter().flatten().map(|b| format!("{:02x}", b.as_u64().unwrap_or(0))).collect::<Vec<_>>().join(" "));
                    return Ok(());
                }
                r
            }
            RawCmd::Report { hex } => {
                let t = open_transport(cli.device.as_deref())?;
                let bytes = parse_hex(&hex)?;
                let n = t.write_report(&bytes)?;
                json!({"wrote": n})
            }
            RawCmd::Listen { seconds } => {
                let t = open_transport(cli.device.as_deref())?;
                let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
                while std::time::Instant::now() < deadline {
                    if let Some(r) = t.read_report(Duration::from_millis(200))? {
                        println!("{}", r.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
                    }
                }
                return Ok(());
            }
        },
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        print_human(&out);
    }
    Ok(())
}

fn open_transport(sel: Option<&str>) -> Result<wooting_hid::Transport> {
    Ok(match sel {
        Some(s) => wooting_hid::Transport::open_matching(s)?,
        None => wooting_hid::Transport::open_first()?,
    })
}

fn print_mapping(v: &Value) {
    println!(
        "slot {} layer {}{}",
        v["slot"],
        v["layer"],
        if v["stored"].as_bool().unwrap_or(false) { "" } else { " (factory default, nothing stored)" }
    );
    let keys = v["keys"].as_object();
    let mut items: Vec<(String, String)> = keys
        .into_iter()
        .flatten()
        .map(|(k, c)| (k.clone(), c["name"].as_str().unwrap_or("?").to_string()))
        .collect();
    items.sort();
    let mut line = String::new();
    for (k, n) in items {
        let cell = format!("{k}={n}");
        if line.len() + cell.len() > 96 {
            println!("{line}");
            line.clear();
        }
        line.push_str(&format!("{cell:<18}"));
    }
    if !line.is_empty() {
        println!("{line}");
    }
}

fn print_human(v: &Value) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                match v {
                    Value::Object(_) | Value::Array(_) => println!("{k}: {}", serde_json::to_string(v).unwrap_or_default()),
                    Value::String(s) => println!("{k}: {s}"),
                    other => println!("{k}: {other}"),
                }
            }
        }
        other => println!("{other}"),
    }
}

fn parse_hex(s: &str) -> Result<Vec<u8>> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace() && *c != ':').collect();
    if clean.len() % 2 != 0 {
        bail!("odd number of hex digits");
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).context("bad hex"))
        .collect()
}

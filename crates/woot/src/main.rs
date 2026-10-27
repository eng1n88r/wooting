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
    /// Show the active profile's raw key mapping matrix.
    Keymap,
    /// Global settings.
    Settings,
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
    let mut call = |method: &str, params: Value| -> Result<Value> {
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
        Cmd::Keymap => {
            let v = call("keymap.get", json!({}))?;
            if !json {
                for row in v["rows"].as_array().into_iter().flatten() {
                    let cells: Vec<String> = row.as_array().into_iter().flatten()
                        .map(|c| match c.as_u64() { Some(255) => " --".into(), Some(n) => format!("{n:3}"), None => "  ?".into() })
                        .collect();
                    println!("{}", cells.join(" "));
                }
                return Ok(());
            }
            v
        }
        Cmd::Settings => call("settings.global.get", json!({}))?,
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

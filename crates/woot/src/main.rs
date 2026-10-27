use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use std::{thread, time::Duration};
use wooting_hid::Transport;
use wooting_proto::{Command, Frame, Rgb};

#[derive(Parser)]
#[command(name = "woot", version, about = "Control Wooting keyboards from the terminal")]
struct Cli {
    /// Select a keyboard by hidraw path or serial number.
    #[arg(long, global = true)]
    device: Option<String>,
    /// Print machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List attached Wooting keyboards.
    Devices,
    /// Send Ping and report the round trip.
    Ping,
    /// Firmware version, serial and device configuration.
    Info,
    /// Send raw commands or reports (for protocol work).
    Raw(RawArgs),
    /// Direct LED control.
    Rgb(RgbArgs),
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
        /// Do not wait for a response.
        #[arg(long)]
        no_reply: bool,
        /// Required for commands that reset or reflash the keyboard.
        #[arg(long)]
        confirm: bool,
    },
    /// Write a raw output report given as hex (first byte = report id).
    Report { hex: String },
    /// Read input reports for N seconds and print them.
    Listen {
        #[arg(default_value_t = 5)]
        seconds: u64,
    },
    /// List all known command ids.
    Commands,
}

#[derive(Args)]
struct RgbArgs {
    #[command(subcommand)]
    cmd: RgbCmd,
}

#[derive(Subcommand)]
enum RgbCmd {
    /// Light every LED with one colour.
    Fill { color: Rgb },
    /// Set one LED by matrix row/column.
    Key { row: u8, col: u8, color: Rgb },
    /// Light LEDs one at a time to discover the matrix layout.
    Identify {
        #[arg(long, default_value_t = 14)]
        cols: u8,
        #[arg(long, default_value_t = 400)]
        delay_ms: u64,
    },
    /// Hand control back to the keyboard's own lighting.
    Release,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();

    if let Cmd::Devices = cli.cmd {
        let devs = wooting_hid::enumerate()?;
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&devs)?);
        } else {
            for d in devs {
                println!(
                    "{:<24} {:04x}:{:04x} {:<10} {:?} usage_page=0x{:04x} serial={}",
                    d.model_name(),
                    d.vid,
                    d.pid,
                    d.path,
                    d.protocol,
                    d.usage_page,
                    d.serial.as_deref().unwrap_or("-")
                );
            }
        }
        return Ok(());
    }

    let t = match &cli.device {
        Some(sel) => Transport::open_matching(sel)?,
        None => Transport::open_first()?,
    };

    match cli.cmd {
        Cmd::Devices => unreachable!(),
        Cmd::Ping => {
            let start = std::time::Instant::now();
            let r = t.command(Command::Ping, [0; 4])?;
            println!(
                "pong from {} in {:?} (status {}, payload {})",
                t.info().model_name(),
                start.elapsed(),
                r.status(),
                hex(r.payload_trimmed())
            );
        }
        Cmd::Info => {
            let mut out = serde_json::Map::new();
            out.insert("model".into(), t.info().model_name().into());
            out.insert("path".into(), t.info().path.clone().into());
            out.insert("protocol".into(), format!("{:?}", t.protocol()).into());
            for cmd in [
                Command::GetVersion,
                Command::GetSerial,
                Command::GetDeviceConfig,
                Command::GetNumberOfKeys,
                Command::GetCurrentKeyboardProfileIndex,
                Command::IsFlashChipConnected,
            ] {
                let entry = match t.command(cmd, [0; 4]) {
                    Ok(r) => serde_json::json!({
                        "status": r.status(),
                        "payload": hex(r.payload_trimmed()),
                    }),
                    Err(e) => serde_json::json!({ "error": e.to_string() }),
                };
                out.insert(cmd.name().to_string(), entry);
            }
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                for (k, v) in &out {
                    println!("{k:<32} {v}");
                }
            }
        }
        Cmd::Raw(RawArgs { cmd }) => match cmd {
            RawCmd::Commands => {
                for c in Command::ALL {
                    println!("{:>2}  {}", *c as u8, c.name());
                }
            }
            RawCmd::Cmd {
                command,
                params,
                no_reply,
                confirm,
            } => {
                let cmd = Command::parse(&command)
                    .with_context(|| format!("unknown command {command:?}"))?;
                if cmd.is_dangerous() && !confirm {
                    bail!("{cmd} resets or reflashes the keyboard; pass --confirm to send it");
                }
                let mut p = [0u8; 4];
                p[..params.len()].copy_from_slice(&params);
                if no_reply {
                    t.send(cmd, p)?;
                    println!("sent {cmd}");
                } else {
                    let r = t.command(cmd, p)?;
                    if cli.json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "command": cmd.name(),
                                "status": r.status(),
                                "payload": hex(r.payload()),
                                "raw": hex(&r.raw),
                            })
                        );
                    } else {
                        println!("{cmd} status={} payload={}", r.status(), hex(r.payload_trimmed()));
                    }
                }
            }
            RawCmd::Report { hex: h } => {
                let bytes = parse_hex(&h)?;
                let n = t.write_report(&bytes)?;
                println!("wrote {n} bytes");
            }
            RawCmd::Listen { seconds } => {
                let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
                while std::time::Instant::now() < deadline {
                    if let Some(r) = t.read_report(Duration::from_millis(200))? {
                        println!("{}", hex(&r));
                    }
                }
            }
        },
        Cmd::Rgb(RgbArgs { cmd }) => match cmd {
            RgbCmd::Fill { color } => {
                t.command(Command::WootDevInit, [0; 4])?;
                t.write_report(&Frame::filled(color).to_v3_report())?;
            }
            RgbCmd::Key { row, col, color } => {
                t.command(Command::WootDevInit, [0; 4])?;
                t.command(Command::WootDevSingleColor, Frame::single_color_params(row, col, color))?;
            }
            RgbCmd::Identify { cols, delay_ms } => {
                t.command(Command::WootDevInit, [0; 4])?;
                for row in 0..wooting_proto::ROWS as u8 {
                    for col in 0..cols {
                        let mut f = Frame::default();
                        f.set(row as usize, col as usize, Rgb::new(255, 255, 255));
                        t.write_report(&f.to_v3_report())?;
                        println!("row {row} col {col}");
                        thread::sleep(Duration::from_millis(delay_ms));
                    }
                }
                t.command(Command::WootDevResetAll, [0; 4])?;
            }
            RgbCmd::Release => {
                t.command(Command::WootDevResetAll, [0; 4])?;
            }
        },
    }
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
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

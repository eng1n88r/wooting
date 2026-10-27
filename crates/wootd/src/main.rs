//! wootd: owns the keyboard and serves it over a Unix socket (JSON-RPC),
//! HTTP (localhost) and, in `mcp` mode, the Model Context Protocol on stdio.

mod http;
mod linker;
mod mcp;
mod rpc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use wooting_core::{api::Api, client::default_socket_path};

#[derive(Parser)]
#[command(name = "wootd", version, about = "Wooting keyboard daemon")]
struct Cli {
    /// Select a keyboard by hidraw path or serial number.
    #[arg(long, global = true)]
    device: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the daemon (default).
    Serve {
        /// Unix socket path for JSON-RPC.
        #[arg(long)]
        socket: Option<PathBuf>,
        /// HTTP listen address.
        #[arg(long, default_value = "127.0.0.1:7357")]
        http: SocketAddr,
        /// Disable the HTTP listener.
        #[arg(long)]
        no_http: bool,
    },
    /// Serve MCP over stdio. Forwards to a running daemon when possible,
    /// otherwise opens the keyboard directly.
    Mcp {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    match cli.cmd.unwrap_or(Cmd::Serve {
        socket: None,
        http: "127.0.0.1:7357".parse().unwrap(),
        no_http: false,
    }) {
        Cmd::Serve {
            socket,
            http,
            no_http,
        } => {
            let api = Arc::new(Api::new(cli.device));
            let socket = socket.unwrap_or_else(default_socket_path);
            let rpc = tokio::spawn(rpc::serve_socket(api.clone(), socket.clone()));
            tokio::spawn(linker::run(api.clone()));
            tracing::info!(socket = %socket.display(), "listening");
            let http_task = if no_http {
                None
            } else {
                tracing::info!(%http, "listening");
                Some(tokio::spawn(http::serve(api.clone(), http)))
            };
            tokio::select! {
                r = rpc => r??,
                r = async { match http_task { Some(t) => t.await?, None => std::future::pending().await } } => r?,
                _ = tokio::signal::ctrl_c() => tracing::info!("shutting down"),
            }
            let _ = std::fs::remove_file(&socket);
            // Give the LEDs back before exit.
            let _ = api.call("rgb.release", serde_json::json!({}));
            Ok(())
        }
        Cmd::Mcp { socket } => {
            let socket = socket.unwrap_or_else(default_socket_path);
            mcp::serve_stdio(socket, cli.device).await
        }
    }
}

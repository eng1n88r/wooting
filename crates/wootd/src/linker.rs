//! Linked profiles: switch the keyboard profile when a matching window gets
//! focus. Listens to Hyprland's event socket (`activewindow>>class,title`).
//!
//! Rules live in `~/.config/wooting/links.toml`:
//!
//! ```toml
//! default = "Typing Profile"        # profile when no rule matches (optional)
//! [[link]]
//! class = "cs2"                      # window class, matched case-insensitively
//! profile = "Rapid Profile"          # profile name or slot number
//! [[link]]
//! title = "Minecraft"                # substring of the window title
//! profile = 1
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::UnixStream,
};
use wooting_core::api::Api;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Links {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default, rename = "link")]
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    pub profile: String,
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wooting/links.toml")
}

pub fn load() -> Links {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(links: &Links) -> Result<()> {
    let path = config_path();
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, toml::to_string_pretty(links)?)?;
    Ok(())
}

fn event_socket() -> Option<PathBuf> {
    let hypr = dirs::runtime_dir()?.join("hypr");
    if let Ok(sig) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
        let p = hypr.join(sig).join(".socket2.sock");
        if p.exists() {
            return Some(p);
        }
    }
    // No signature in the environment (e.g. a systemd unit started before the
    // session exported it): take the newest instance directory that has a socket.
    let mut dirs: Vec<_> = std::fs::read_dir(&hypr)
        .ok()?
        .flatten()
        .map(|e| e.path().join(".socket2.sock"))
        .filter(|p| p.exists())
        .collect();
    dirs.sort_by_key(|p| p.metadata().and_then(|m| m.modified()).ok());
    dirs.pop()
}

/// Resolve a profile name or slot number to a slot via the API.
async fn resolve(api: &Arc<Api>, profile: &str) -> Option<u8> {
    if let Ok(n) = profile.parse::<u8>() {
        return Some(n);
    }
    let list = crate::rpc::call_blocking(api, "profile.list".into(), json!({}))
        .await
        .ok()?;
    list.as_array()?
        .iter()
        .find(|p| {
            p["name"]
                .as_str()
                .map(|n| n.eq_ignore_ascii_case(profile))
                .unwrap_or(false)
        })
        .and_then(|p| p["slot"].as_u64())
        .map(|s| s as u8)
}

/// Run forever: reconnects to Hyprland if the socket goes away.
pub async fn run(api: Arc<Api>) -> Result<()> {
    let Some(path) = event_socket() else {
        tracing::info!("no Hyprland event socket; linked profiles disabled");
        return Ok(());
    };
    tracing::info!(path = %path.display(), "linked profiles: watching Hyprland focus");
    let mut current: Option<u8> = None;
    loop {
        let stream = match UnixStream::connect(&path).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "hyprland socket; retrying in 5 s");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Some(rest) = line.strip_prefix("activewindow>>") else {
                continue;
            };
            let (class, title) = rest.split_once(',').unwrap_or((rest, ""));
            let links = load();
            if links.links.is_empty() {
                continue;
            }
            let matched = links.links.iter().find(|l| {
                let class_ok = l
                    .class
                    .as_deref()
                    .map(|c| class.eq_ignore_ascii_case(c))
                    .unwrap_or(true);
                let title_ok = l
                    .title
                    .as_deref()
                    .map(|t| title.to_lowercase().contains(&t.to_lowercase()))
                    .unwrap_or(true);
                (l.class.is_some() || l.title.is_some()) && class_ok && title_ok
            });
            let want = match matched {
                Some(l) => l.profile.clone(),
                None => match &links.default {
                    Some(d) => d.clone(),
                    None => continue,
                },
            };
            let Some(slot) = resolve(&api, &want).await else {
                tracing::warn!(profile = %want, "linked profile not found");
                continue;
            };
            if current == Some(slot) {
                continue;
            }
            match crate::rpc::call_blocking(
                &api,
                "profile.activate".into(),
                json!({"slot": slot, "discard": true}),
            )
            .await
            {
                Ok(_) => {
                    tracing::info!(class, slot, "linked profile activated");
                    current = Some(slot);
                }
                Err(e) => tracing::warn!(error = %e.message, "linked profile switch failed"),
            }
        }
        tracing::warn!("hyprland socket closed; reconnecting");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Link rules as JSON for the API.
pub fn links_json() -> serde_json::Value {
    serde_json::to_value(load()).unwrap_or_default()
}

pub fn add(class: Option<String>, title: Option<String>, profile: String) -> Result<Links> {
    let class = class.filter(|c| !c.trim().is_empty());
    let title = title.filter(|t| !t.trim().is_empty());
    if class.is_none() && title.is_none() {
        anyhow::bail!("a rule needs a window class and/or a title fragment");
    }
    let mut links = load();
    links
        .links
        .retain(|l| !(l.class == class && l.title == title));
    links.links.push(Link {
        class,
        title,
        profile,
    });
    save(&links).context("saving links.toml")?;
    Ok(links)
}

pub fn remove(index: usize) -> Result<Links> {
    let mut links = load();
    if index < links.links.len() {
        links.links.remove(index);
    }
    save(&links)?;
    Ok(links)
}

pub fn set_default(profile: Option<String>) -> Result<Links> {
    let mut links = load();
    links.default = profile;
    save(&links)?;
    Ok(links)
}

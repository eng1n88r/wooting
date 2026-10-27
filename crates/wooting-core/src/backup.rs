//! Dump everything readable from the keyboard to a JSON file so a profile
//! can be inspected or restored after an experiment goes wrong.

use crate::{Keyboard, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use wooting_proto::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct Backup {
    pub created: String,
    pub model: String,
    pub serial: Option<String>,
    pub firmware: Option<String>,
    /// Raw responses keyed by command name (and slot for per-profile reads).
    pub responses: BTreeMap<String, Vec<u8>>,
}

const SNAPSHOT_COMMANDS: &[Command] = &[
    Command::GetVersion,
    Command::GetSerial,
    Command::GetDeviceConfig,
    Command::GetCurrentKeyboardProfileIndex,
    Command::GetRgbProfileColorsPart1,
    Command::GetRgbProfileColorsPart2,
    Command::GetRgbProfileCore,
    Command::GetGlobalSettings,
    Command::GetMappingProfile,
    Command::GetKeyboardProfile,
    Command::GetMainMappingProfile,
    Command::GetFunctionMappingProfile,
    Command::GetDksProfile,
    Command::GetAkcProfile,
    Command::GetActuationProfile,
    Command::GetRapidTriggerProfile,
    Command::GetGamepadMapping,
    Command::GetGamepadProfile,
    Command::GetRgbLayer,
    Command::GetFlashStats,
];

pub fn backup_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wooting/backups")
}

pub fn snapshot(kb: &Keyboard) -> Result<Backup> {
    let summary = kb.summary();
    let mut responses = BTreeMap::new();
    for cmd in SNAPSHOT_COMMANDS {
        if let Ok(r) = kb.transport().command(*cmd, [0; 4]) {
            responses.insert(cmd.name().to_string(), r.raw);
        }
    }
    for slot in 0..8u8 {
        match kb
            .transport()
            .command(Command::GetProfileMetadata, [0, 0, 0, slot])
        {
            Ok(r) if !r.data().is_empty() => {
                responses.insert(format!("GetProfileMetadata/{slot}"), r.raw);
            }
            _ => break,
        }
    }
    Ok(Backup {
        created: chrono::Local::now().to_rfc3339(),
        model: summary.model,
        serial: summary.serial,
        firmware: summary.firmware,
        responses,
    })
}

/// Snapshot and write to the backup directory; returns the file path.
pub fn save(kb: &Keyboard) -> Result<PathBuf> {
    let b = snapshot(kb)?;
    let dir = backup_dir();
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("{stamp}.json"));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&b).map_err(std::io::Error::other)?,
    )?;
    Ok(path)
}

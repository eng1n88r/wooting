//! Editable copy of a profile with undo/redo.
//!
//! The keyboard only reports what is in flash, so pending (RAM-only) edits
//! must be tracked host-side. A [`Draft`] holds the full profile as loaded
//! from flash plus the edited state; every edit snapshots the previous state
//! for undo and is pushed to the keyboard's RAM so it takes effect at once.
//! `save` writes everything into the flash slot, `discard` reloads the slot.

use crate::{Keyboard, Result};
use serde::{Deserialize, Serialize};
use wooting_proto::{
    pb, report,
    report::{flags_persist, FLAGS_RAM},
    Command, Frame, Message, COLS, ROWS,
};

pub type Rows = [[u8; COLS]; ROWS];

/// Everything that makes up one onboard profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfileData {
    pub meta: pb::ProfileMetadata,
    pub keyboard: pb::KeyboardProfile,
    pub per_key: pb::PerKey,
    pub rt_keys: pb::RtKeys,
    /// Layers 0..4; `None` = factory default for that layer.
    pub mapping: Vec<Option<Rows>>,
    pub rgb: Frame,
    pub core: pb::RgbProfileCore,
    pub akc: pb::AdvancedKeys,
    /// Analog curve (report 0x13) kept verbatim.
    pub curve: Vec<u8>,
}

impl ProfileData {
    pub fn mapping_layer(&self, layer: u8, layout: &wooting_proto::Layout) -> Rows {
        self.mapping
            .get(layer as usize)
            .and_then(|m| *m)
            .unwrap_or_else(|| layout.default_mapping(layer))
    }
}

impl Keyboard {
    /// Read a whole profile from flash.
    pub fn read_profile(&self, slot: u8) -> Result<ProfileData> {
        let meta = self
            .profile_metadata(slot)?
            .ok_or(crate::Error::NoSuchProfile(slot))?;
        let mut mapping = Vec::new();
        for layer in 0..4u8 {
            mapping.push(self.mapping(slot, layer)?);
        }
        let curve = self
            .transport()
            .command(Command::GetGamepadProfile, [0, 0, 0, slot])
            .map(|r| r.data().to_vec())
            .unwrap_or_default();
        Ok(ProfileData {
            meta,
            keyboard: self.keyboard_profile(slot)?,
            per_key: self.per_key_raw(slot)?,
            rt_keys: self.rt_keys_raw(slot)?,
            mapping,
            rgb: self.read_rgb_profile(slot)?,
            core: self.rgb_core(slot)?,
            akc: self.advanced_keys_raw(slot)?,
            curve,
        })
    }

    /// Push a profile into the keyboard's RAM (active profile) or flash slot.
    /// Mirrors Wootility's save order.
    pub fn write_profile(&self, data: &ProfileData, target: WriteTarget) -> Result<()> {
        let flags = match target {
            WriteTarget::Ram => FLAGS_RAM,
            WriteTarget::Flash(slot) => flags_persist(slot),
        };
        let rows = |range: std::ops::Range<usize>| pb::RgbRows {
            rows: range
                .map(|r| pb::rgb_rows::Row {
                    colors: data.rgb.0[r].iter().map(|c| c.to_565() as u32).collect(),
                })
                .collect(),
        };
        // RGB goes through RAM and SaveRgbProfile.
        self.report_pb(report::kind::RGB_PROFILE_CORE, FLAGS_RAM, &data.core)?;
        self.report_pb(
            report::kind::RGB_PROFILE_COLORS_PART1,
            FLAGS_RAM,
            &rows(0..3),
        )?;
        self.report_pb(
            report::kind::RGB_PROFILE_COLORS_PART2,
            FLAGS_RAM,
            &rows(3..6),
        )?;
        if let WriteTarget::Flash(slot) = target {
            self.cmd(Command::SaveRgbProfile, [0, 0, 0, slot])?;
        }
        self.report_pb(report::kind::RT_KEYS, flags, &data.rt_keys)?;
        self.report_pb(report::kind::KEYBOARD_PROFILE, flags, &data.keyboard)?;
        if !data.curve.is_empty() {
            let bytes = report::build(
                self.transport().protocol(),
                report::kind::ANALOG_CURVE,
                flags,
                &data.curve,
            )
            .ok_or_else(|| crate::Error::Other("curve too large".into()))?;
            self.transport().write_report(&bytes)?;
        }
        self.report_pb(
            report::kind::UNKNOWN_12,
            flags,
            &pb::MappingProfile::default(),
        )?;
        for (layer, rows) in data.mapping.iter().enumerate() {
            let payload = rows
                .map(|r| {
                    pb::MappingProfile {
                        rows: r
                            .iter()
                            .map(|row| pb::mapping_profile::Row {
                                codes: row.to_vec(),
                            })
                            .collect(),
                    }
                    .encode_to_vec()
                })
                .unwrap_or_default();
            let bytes =
                report::build_mapping(self.transport().protocol(), flags, layer as u8, &payload)
                    .ok_or_else(|| crate::Error::Other("mapping too large".into()))?;
            self.transport().write_report(&bytes)?;
        }
        self.report_pb(report::kind::PER_KEY, flags, &data.per_key)?;
        // Advanced keys go through RAM and SaveAkcProfile.
        self.report_pb(report::kind::ADVANCED_KEYS, FLAGS_RAM, &data.akc)?;
        if let WriteTarget::Flash(slot) = target {
            self.cmd(Command::SaveAkcProfile, [0, 0, 0, slot])?;
        }
        self.report_pb(report::kind::PROFILE_METADATA, flags, &data.meta)?;
        if let WriteTarget::Flash(slot) = target {
            let _ = self.cmd(Command::PostSaveProfile, [0, 0, 0, slot]);
        }
        Ok(())
    }
}

impl Keyboard {
    /// Initialise `slot` (Wootility's CreateProfile sequence) and write `data` into it.
    pub fn create_profile(&self, slot: u8, data: &ProfileData) -> Result<()> {
        self.cmd(Command::CreateProfile, [0, 0, 0, slot])?;
        // Four zeroed 510-byte blobs (extra RGB layers), as Wootility sends them.
        for chunk in 0..4u8 {
            let flags = (chunk << 5) | (slot << 1);
            let bytes = report::build(
                self.transport().protocol(),
                report::kind::PROFILE_BLOB,
                flags,
                &[0u8; 504],
            )
            .ok_or_else(|| crate::Error::Other("blob too large".into()))?;
            self.transport().write_report(&bytes)?;
        }
        self.write_profile(data, WriteTarget::Flash(slot))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum WriteTarget {
    Ram,
    Flash(u8),
}

/// An in-progress edit of one profile slot.
#[derive(Debug, Clone)]
pub struct Draft {
    pub slot: u8,
    pub base: ProfileData,
    pub data: ProfileData,
    undo: Vec<ProfileData>,
    redo: Vec<ProfileData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftStatus {
    pub slot: u8,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

impl Draft {
    pub fn load(kb: &Keyboard, slot: u8) -> Result<Draft> {
        let base = kb.read_profile(slot)?;
        Ok(Draft {
            slot,
            data: base.clone(),
            base,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn status(&self) -> DraftStatus {
        DraftStatus {
            slot: self.slot,
            dirty: self.data != self.base,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
        }
    }

    /// Apply `edit`, snapshotting for undo, and push the result to RAM when
    /// this draft is the active profile.
    pub fn edit(
        &mut self,
        kb: &Keyboard,
        edit: impl FnOnce(&mut ProfileData) -> Result<()>,
    ) -> Result<()> {
        let before = self.data.clone();
        edit(&mut self.data)?;
        if self.data == before {
            return Ok(());
        }
        self.undo.push(before);
        self.redo.clear();
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.push_ram(kb)
    }

    fn push_ram(&self, kb: &Keyboard) -> Result<()> {
        if kb.active_profile()? == self.slot {
            kb.write_profile(&self.data, WriteTarget::Ram)?;
        }
        Ok(())
    }

    pub fn undo(&mut self, kb: &Keyboard) -> Result<bool> {
        let Some(prev) = self.undo.pop() else {
            return Ok(false);
        };
        self.redo.push(std::mem::replace(&mut self.data, prev));
        self.push_ram(kb)?;
        Ok(true)
    }

    pub fn redo(&mut self, kb: &Keyboard) -> Result<bool> {
        let Some(next) = self.redo.pop() else {
            return Ok(false);
        };
        self.undo.push(std::mem::replace(&mut self.data, next));
        self.push_ram(kb)?;
        Ok(true)
    }

    /// Commit to flash (and RAM if active); the draft becomes clean.
    pub fn save(&mut self, kb: &Keyboard) -> Result<()> {
        kb.write_profile(&self.data, WriteTarget::Flash(self.slot))?;
        if kb.active_profile()? == self.slot {
            kb.write_profile(&self.data, WriteTarget::Ram)?;
        }
        self.base = self.data.clone();
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }

    /// Throw the edits away and reload the slot from flash.
    pub fn discard(&mut self, kb: &Keyboard) -> Result<()> {
        if kb.active_profile()? == self.slot {
            kb.cmd(Command::ReloadProfile, [0, 0, 0, self.slot])?;
        }
        self.base = kb.read_profile(self.slot)?;
        self.data = self.base.clone();
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }
}

// ----- edits on ProfileData (pure, no I/O) ---------------------------------

use crate::keyboard::{code_to_mm, mm_to_code, AdvancedKey, KeyboardSettings};
use crate::Error;
use wooting_proto::{keycode, rgb::matrix_id, Layout, Rgb};

fn key_cells<'a>(layout: &'a Layout, id: &str) -> Result<&'a wooting_proto::Key> {
    layout
        .key(id)
        .ok_or_else(|| Error::UnknownKey(id.to_string()))
}

fn key_for_matrix(layout: &Layout, id: u8) -> String {
    layout
        .key_at_cell(id >> 5, id & 0x1f)
        .map(|k| k.id.clone())
        .unwrap_or_else(|| format!("cell_{}_{}", id >> 5, id & 0x1f))
}

impl ProfileData {
    pub fn name(&self) -> &str {
        &self.meta.name
    }

    pub fn settings(&self) -> KeyboardSettings {
        KeyboardSettings::from_pb(&self.keyboard)
    }

    pub fn set_settings(&mut self, s: &KeyboardSettings) {
        s.apply(&mut self.keyboard);
    }

    pub fn rename(&mut self, name: Option<&str>, color: Option<Rgb>) {
        if let Some(n) = name {
            self.meta.name = n.to_string();
        }
        if let Some(c) = color {
            self.meta.color = Some(pb::profile_metadata::Color {
                r: Some(c.r as u32),
                g: Some(c.g as u32),
                b: Some(c.b as u32),
            });
        }
    }

    /// Set the codes of `keys` on `layer`; the layer becomes stored (unlinked).
    pub fn set_keys(&mut self, layout: &Layout, layer: u8, keys: &[(String, u8)]) -> Result<()> {
        while self.mapping.len() <= layer as usize {
            self.mapping.push(None);
        }
        let mut rows = self.mapping_layer(layer, layout);
        for (id, code) in keys {
            for c in &key_cells(layout, id)?.cells {
                rows[c[0] as usize][c[1] as usize] = *code;
            }
        }
        self.mapping[layer as usize] = Some(rows);
        Ok(())
    }

    pub fn set_layer(&mut self, layer: u8, rows: Option<Rows>) {
        while self.mapping.len() <= layer as usize {
            self.mapping.push(None);
        }
        self.mapping[layer as usize] = rows;
    }

    pub fn per_key_actuation(&self, layout: &Layout) -> Vec<(String, f32)> {
        self.per_key
            .actuation
            .iter()
            .flat_map(|e| e.packed.iter().copied())
            .map(|v| {
                (
                    key_for_matrix(layout, (v & 0xff) as u8),
                    code_to_mm(v >> 16),
                )
            })
            .collect()
    }

    pub fn set_per_key_actuation(
        &mut self,
        layout: &Layout,
        changes: &[(String, Option<f32>)],
    ) -> Result<()> {
        for (id, mm) in changes {
            for c in &key_cells(layout, id)?.cells {
                let mid = matrix_id(c[0], c[1]) as u32;
                self.per_key
                    .actuation
                    .retain(|e| e.packed.first().map(|v| v & 0xff) != Some(mid));
                if let Some(mm) = mm {
                    self.per_key.actuation.push(pb::per_key::Entry {
                        packed: vec![(mm_to_code(*mm) << 16) | mid],
                    });
                }
            }
        }
        Ok(())
    }

    pub fn rt_keys(&self, layout: &Layout) -> Vec<String> {
        self.rt_keys
            .keys
            .as_ref()
            .map(|k| {
                k.key
                    .iter()
                    .map(|m| key_for_matrix(layout, *m as u8))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set_rt_keys(&mut self, layout: &Layout, changes: &[(String, bool)]) -> Result<()> {
        let keys = self.rt_keys.keys.get_or_insert_with(Default::default);
        for (id, on) in changes {
            for c in &key_cells(layout, id)?.cells {
                let mid = matrix_id(c[0], c[1]) as u32;
                keys.key.retain(|k| *k != mid);
                if *on {
                    keys.key.push(mid);
                }
            }
        }
        keys.key.sort_unstable();
        keys.key.dedup();
        keys.enabled = Some(1);
        Ok(())
    }

    pub fn brightness(&self) -> u8 {
        self.core
            .payload
            .as_ref()
            .and_then(|p| p.brightness)
            .unwrap_or(0) as u8
    }

    pub fn set_brightness(&mut self, b: u8) {
        let p = self
            .core
            .payload
            .get_or_insert_with(|| pb::rgb_profile_core::Payload {
                a: Some(0xFFFF),
                b: Some(0xFFFF),
                ..Default::default()
            });
        p.brightness = Some(b as u32);
    }

    pub fn effects(&self) -> Vec<pb::EffectLayer> {
        self.core
            .effects
            .as_ref()
            .map(|e| e.layers.clone())
            .unwrap_or_default()
    }

    pub fn set_effects(&mut self, layers: Vec<pb::EffectLayer>) {
        self.core.effects = Some(pb::Effects { layers });
    }

    pub fn set_key_colors(&mut self, layout: &Layout, colors: &[(String, Rgb)]) -> Result<()> {
        for (id, c) in colors {
            for cell in &key_cells(layout, id)?.cells {
                self.rgb.set(cell[0] as usize, cell[1] as usize, *c);
            }
        }
        Ok(())
    }

    pub fn advanced_keys(&self, layout: &Layout) -> Vec<AdvancedKey> {
        self.akc
            .entries
            .iter()
            .map(|e| {
                let key = key_for_matrix(layout, e.key.unwrap_or(0) as u8);
                let name = |c: Option<u32>| c.map(|c| keycode::name(c as u8));
                let other = |m: Option<u32>| m.map(|m| key_for_matrix(layout, m as u8));
                let (kind, tap, hold, other_key, codes, mode) = if let Some(m) = &e.mod_tap {
                    ("mod_tap", name(m.tap), name(m.hold), None, None, None)
                } else if let Some(t) = &e.toggle {
                    ("toggle", name(t.code), None, None, None, None)
                } else if let Some(r) = &e.rappy_snappy {
                    ("rappy_snappy", None, None, other(r.other_key), None, None)
                } else if let Some(sd) = &e.socd {
                    ("socd", None, None, other(sd.other_key), None, sd.mode)
                } else if let Some(d) = &e.dks {
                    let codes: Vec<String> = d.codes.iter().map(|c| keycode::name(*c)).collect();
                    (
                        "dks",
                        None,
                        None,
                        None,
                        Some((codes, d.masks.clone())),
                        None,
                    )
                } else {
                    ("unknown", None, None, None, None, None)
                };
                AdvancedKey {
                    key,
                    kind: kind.to_string(),
                    tap,
                    hold,
                    other_key,
                    dks_codes: codes.as_ref().map(|c| c.0.clone()),
                    dks_masks: codes.map(|c| c.1),
                    mode,
                }
            })
            .collect()
    }

    pub fn set_advanced_key(
        &mut self,
        layout: &Layout,
        key_id: &str,
        mut entry: pb::advanced_keys::Entry,
    ) -> Result<()> {
        let cell = key_cells(layout, key_id)?.cells[0];
        let id = matrix_id(cell[0], cell[1]) as u32;
        entry.key = Some(id);
        entry.f9.get_or_insert(0);
        if self.akc.entries.len() >= 40 && !self.akc.entries.iter().any(|e| e.key == Some(id)) {
            return Err(Error::Other(
                "the keyboard holds at most 40 advanced keys".into(),
            ));
        }
        match self.akc.entries.iter_mut().find(|e| e.key == Some(id)) {
            Some(e) => *e = entry,
            None => self.akc.entries.push(entry),
        }
        self.akc.f2.get_or_insert(0);
        self.akc.count = Some(self.akc.entries.len() as u32);
        Ok(())
    }

    pub fn remove_advanced_key(&mut self, layout: &Layout, key_id: &str) -> Result<()> {
        let cell = key_cells(layout, key_id)?.cells[0];
        let id = matrix_id(cell[0], cell[1]) as u32;
        self.akc.entries.retain(|e| e.key != Some(id));
        self.akc.f2.get_or_insert(0);
        self.akc.count = Some(self.akc.entries.len() as u32);
        Ok(())
    }

    pub fn matrix_of(&self, layout: &Layout, key_id: &str) -> Result<u32> {
        let cell = key_cells(layout, key_id)?.cells[0];
        Ok(matrix_id(cell[0], cell[1]) as u32)
    }
}

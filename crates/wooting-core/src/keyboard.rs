use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{thread, time::Duration};
use wooting_hid::{DeviceInfo, Transport};
use wooting_proto::{
    keycode, pb, report,
    report::{flags_persist, FLAGS_GLOBAL, FLAGS_RAM},
    status_name, Command, Frame, Layout, Message, Response, Rgb, COLS, ROWS, STATUS_OK,
};

/// Distances in KeyboardProfile: `0x4000 | round(mm / 4.0 * 16383)`.
pub fn mm_to_code(mm: f32) -> u32 {
    let x = (mm.clamp(0.0, 4.0) / 4.0 * 16383.0).round() as u32;
    0x4000 | x
}

pub fn code_to_mm(code: u32) -> f32 {
    ((code & 0x3fff) as f32 / 16383.0 * 4.0 * 100.0).round() / 100.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedKey {
    pub key: String,
    /// `mod_tap`, `toggle`, `rappy_snappy`, `socd`, `dks` or `unknown`.
    pub kind: String,
    pub tap: Option<String>,
    pub hold: Option<String>,
    /// Partner key for Rappy Snappy / SOCD.
    pub other_key: Option<String>,
    /// DKS: four bindings (code names) and their press-phase masks.
    pub dks_codes: Option<Vec<String>>,
    pub dks_masks: Option<Vec<u8>>,
    /// SOCD mode.
    pub mode: Option<u32>,
}

/// Human view of [`pb::KeyboardProfile`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardSettings {
    pub actuation_mm: f32,
    pub rapid_trigger: bool,
    pub rt_press_mm: f32,
    /// `None` = same as press (split sensitivity off).
    pub rt_release_mm: Option<f32>,
    /// Rapid trigger keeps re-arming until the key is fully released.
    pub continuous_rt: bool,
    pub tachyon: bool,
}

impl KeyboardSettings {
    pub fn from_pb(p: &pb::KeyboardProfile) -> KeyboardSettings {
        let d = p.payload.unwrap_or_default();
        let g = |v: Option<u32>| v.unwrap_or(0);
        KeyboardSettings {
            actuation_mm: code_to_mm(g(d.actuation)),
            rapid_trigger: g(d.rapid_trigger) != 0,
            rt_press_mm: code_to_mm(g(d.rt_press)),
            rt_release_mm: (g(d.rt_release) != 0).then(|| code_to_mm(g(d.rt_release))),
            continuous_rt: d.rt_ends_at_actuation == Some(0),
            tachyon: g(d.tachyon) != 0,
        }
    }

    /// Apply onto an existing message so unknown fields keep their values.
    pub fn apply(&self, p: &mut pb::KeyboardProfile) {
        let d = p.payload.get_or_insert_with(Default::default);
        d.actuation = Some(mm_to_code(self.actuation_mm));
        d.f2.get_or_insert(1);
        d.rapid_trigger = Some(self.rapid_trigger as u32);
        d.rt_press = Some(mm_to_code(self.rt_press_mm));
        d.tachyon = Some(self.tachyon as u32);
        d.f6.get_or_insert(2);
        d.rt_ends_at_actuation = Some(if self.continuous_rt { 0 } else { 1 });
        // The firmware omits rt_release entirely unless split sensitivity is on.
        d.rt_release = self.rt_release_mm.map(mm_to_code);
        d.f9 = Some(self.tachyon as u32);
    }
}

/// Delay between ActivateProfile and ReloadProfile (empirical, from wootswitch).
const PROFILE_SWITCH_SETTLE: Duration = Duration::from_millis(100);
const MAX_PROFILE_SLOTS: u8 = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub model: String,
    pub path: String,
    pub serial: Option<String>,
    pub firmware: Option<String>,
    pub protocol: wooting_proto::Protocol,
    pub layout: String,
    pub direct_rgb: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub slot: u8,
    pub name: String,
    pub color: Rgb,
    pub icon: String,
    pub active: bool,
}

/// An open keyboard plus the host-side state we keep for it.
pub struct Keyboard {
    t: Transport,
    layout: Layout,
    /// True once WootDevInit has been sent and we own the LEDs.
    direct: bool,
    /// Last frame pushed in direct mode.
    frame: Frame,
}

impl Keyboard {
    pub fn open_first() -> Result<Keyboard> {
        Ok(Self::from_transport(Transport::open_first()?))
    }

    pub fn open_matching(selector: &str) -> Result<Keyboard> {
        Ok(Self::from_transport(Transport::open_matching(selector)?))
    }

    pub fn open(info: DeviceInfo) -> Result<Keyboard> {
        Ok(Self::from_transport(Transport::open(info)?))
    }

    fn from_transport(t: Transport) -> Keyboard {
        let layout = Layout::for_model(t.info().model_name());
        Keyboard {
            t,
            layout,
            direct: false,
            frame: Frame::default(),
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn transport(&self) -> &Transport {
        &self.t
    }

    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    pub fn is_direct(&self) -> bool {
        self.direct
    }

    // ----- commands -------------------------------------------------------

    /// Send a command and require a success status.
    pub fn cmd(&self, command: Command, params: [u8; 4]) -> Result<Response> {
        let r = self.t.command(command, params)?;
        if r.status() != STATUS_OK {
            return Err(Error::Status {
                command,
                status: r.status(),
                name: status_name(r.status()),
            });
        }
        Ok(r)
    }

    /// Send a command and decode its protobuf payload.
    pub fn cmd_pb<M: Message + Default>(&self, command: Command, params: [u8; 4]) -> Result<M> {
        let r = self.cmd(command, params)?;
        Ok(M::decode(r.data())?)
    }

    /// Write a protobuf-carrying output report with the given flags
    /// ([`FLAGS_RAM`], [`flags_persist`], [`FLAGS_GLOBAL`]).
    pub fn report_pb<M: Message>(&self, kind: u8, flags: u8, msg: &M) -> Result<()> {
        let payload = msg.encode_to_vec();
        let bytes = report::build(self.t.protocol(), kind, flags, &payload)
            .ok_or_else(|| Error::Other(format!("payload of {} bytes too large", payload.len())))?;
        self.t.write_report(&bytes)?;
        Ok(())
    }

    // ----- info -----------------------------------------------------------

    pub fn firmware_version(&self) -> Result<String> {
        let r = self.cmd(Command::GetVersion, [0; 4])?;
        let d = r.data();
        Ok(match d {
            [a, b, c, ..] => format!("{a}.{b}.{c}"),
            _ => d
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join("."),
        })
    }

    pub fn summary(&self) -> DeviceSummary {
        let info = self.t.info();
        DeviceSummary {
            model: info.model_name().to_string(),
            path: info.path.clone(),
            serial: info.serial.clone(),
            firmware: self.firmware_version().ok(),
            protocol: info.protocol,
            layout: self.layout.name.clone(),
            direct_rgb: self.direct,
        }
    }

    // ----- profiles -------------------------------------------------------

    pub fn active_profile(&self) -> Result<u8> {
        let r = self.cmd(Command::GetCurrentKeyboardProfileIndex, [0; 4])?;
        r.data()
            .first()
            .copied()
            .ok_or_else(|| Error::Other("empty profile index".into()))
    }

    pub fn profile_metadata(&self, slot: u8) -> Result<Option<pb::ProfileMetadata>> {
        let r = self.cmd(Command::GetProfileMetadata, [0, 0, 0, slot])?;
        if r.data().is_empty() {
            return Ok(None);
        }
        Ok(Some(pb::ProfileMetadata::decode(r.data())?))
    }

    pub fn profiles(&self) -> Result<Vec<ProfileInfo>> {
        let active = self.active_profile().ok();
        let mut out = Vec::new();
        for slot in 0..MAX_PROFILE_SLOTS {
            let Some(m) = self.profile_metadata(slot)? else {
                break;
            };
            let c = m.color.unwrap_or_default();
            out.push(ProfileInfo {
                slot,
                name: m.name,
                color: Rgb::new(
                    c.r.unwrap_or(0) as u8,
                    c.g.unwrap_or(0) as u8,
                    c.b.unwrap_or(0) as u8,
                ),
                icon: m.icon,
                active: active == Some(slot),
            });
        }
        Ok(out)
    }

    pub fn activate_profile(&self, slot: u8) -> Result<()> {
        if self.profile_metadata(slot)?.is_none() {
            return Err(Error::NoSuchProfile(slot));
        }
        self.cmd(Command::WootDevInit, [0; 4])?;
        self.cmd(Command::ActivateProfile, [0, 0, 0, slot])?;
        thread::sleep(PROFILE_SWITCH_SETTLE);
        self.cmd(Command::ReloadProfile, [0, 0, 0, slot])?;
        thread::sleep(PROFILE_SWITCH_SETTLE);
        Ok(())
    }

    // ----- direct (volatile) lighting ------------------------------------

    pub fn enter_direct(&mut self) -> Result<()> {
        if !self.direct {
            self.cmd(Command::WootDevInit, [0; 4])?;
            self.direct = true;
        }
        Ok(())
    }

    /// Push a whole frame to the LEDs.
    pub fn set_frame(&mut self, frame: Frame) -> Result<()> {
        self.enter_direct()?;
        self.t.write_report(&frame.to_v3_report())?;
        self.frame = frame;
        Ok(())
    }

    pub fn fill(&mut self, color: Rgb) -> Result<()> {
        self.set_frame(Frame::filled(color))
    }

    /// Set the colour of one key (all of its cells) and push the frame.
    pub fn set_key(&mut self, key_id: &str, color: Rgb) -> Result<()> {
        let key = self
            .layout
            .key(key_id)
            .ok_or_else(|| Error::UnknownKey(key_id.to_string()))?;
        let mut frame = self.frame;
        for c in &key.cells {
            frame.set(c[0] as usize, c[1] as usize, color);
        }
        self.set_frame(frame)
    }

    /// Set several keys at once, then push one frame.
    pub fn set_keys(&mut self, colors: &[(String, Rgb)]) -> Result<()> {
        let mut frame = self.frame;
        for (id, color) in colors {
            let key = self
                .layout
                .key(id)
                .ok_or_else(|| Error::UnknownKey(id.clone()))?;
            for c in &key.cells {
                frame.set(c[0] as usize, c[1] as usize, *color);
            }
        }
        self.set_frame(frame)
    }

    /// Give the LEDs back to the keyboard's own lighting profile.
    pub fn release(&mut self) -> Result<()> {
        if self.direct {
            self.cmd(Command::WootDevResetAll, [0; 4])?;
            self.direct = false;
        }
        Ok(())
    }

    // ----- persistent lighting profile -----------------------------------

    /// Slot used when the caller does not name one: the active keyboard
    /// profile (RGB profile slots pair with keyboard profile slots).
    pub fn default_rgb_slot(&self) -> u8 {
        self.active_profile().unwrap_or(0)
    }

    /// Read the colours stored in RGB profile `slot`.
    pub fn read_rgb_profile(&self, slot: u8) -> Result<Frame> {
        let upper: pb::RgbRows = self.cmd_pb(Command::GetRgbProfileColorsPart1, [0, 0, 0, slot])?;
        let lower: pb::RgbRows = self.cmd_pb(Command::GetRgbProfileColorsPart2, [0, 0, 0, slot])?;
        let mut frame = Frame::default();
        for (ri, row) in upper.rows.iter().chain(lower.rows.iter()).enumerate() {
            for (ci, v) in row.colors.iter().enumerate() {
                frame.set(ri, ci, Rgb::from_565(*v as u16));
            }
        }
        Ok(frame)
    }

    /// Store `frame` in RGB profile `slot` (flash write).
    ///
    /// Observed on 60HE+ fw 2.14 and in Wootility captures: the colour-row
    /// reports update the active profile's RGB RAM, and SaveRgbProfile(p3 =
    /// slot) commits that RAM into the slot. Saving without first sending
    /// rows therefore copies the active profile's colours into the slot.
    pub fn write_rgb_profile(&self, frame: &Frame, slot: u8) -> Result<()> {
        self.write_rgb_profile_with(frame, None, slot)
    }

    /// Like [`write_rgb_profile`](Self::write_rgb_profile) but also sets the
    /// stored brightness in the same commit. SaveRgbProfile copies the whole
    /// RGB RAM (rows and core) into the slot, so both must be staged first.
    pub fn write_rgb_profile_with(
        &self,
        frame: &Frame,
        brightness: Option<u8>,
        slot: u8,
    ) -> Result<()> {
        let rows = |range: std::ops::Range<usize>| pb::RgbRows {
            rows: range
                .map(|r| pb::rgb_rows::Row {
                    colors: frame.0[r].iter().map(|c| c.to_565() as u32).collect(),
                })
                .collect(),
        };
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
        if let Some(b) = brightness {
            let mut core: pb::RgbProfileCore =
                self.cmd_pb(Command::GetRgbProfileCore, [0, 0, 0, slot])?;
            let p = core
                .payload
                .get_or_insert_with(|| pb::rgb_profile_core::Payload {
                    a: Some(0xFFFF),
                    b: Some(0xFFFF),
                    ..Default::default()
                });
            p.brightness = Some(b as u32);
            self.report_pb(report::kind::RGB_PROFILE_CORE, FLAGS_RAM, &core)?;
        }
        self.cmd(Command::SaveRgbProfile, [0, 0, 0, slot])?;
        self.refresh_rgb();
        Ok(())
    }

    /// RefreshRgbColors is "unsupported" on 60HE+ fw 2.14; ignore that.
    fn refresh_rgb(&self) {
        if let Err(e) = self.cmd(Command::RefreshRgbColors, [0; 4]) {
            tracing::debug!(error = %e, "RefreshRgbColors ignored");
        }
    }

    pub fn brightness(&self, slot: u8) -> Result<u8> {
        let core: pb::RgbProfileCore = self.cmd_pb(Command::GetRgbProfileCore, [0, 0, 0, slot])?;
        Ok(core.payload.and_then(|p| p.brightness).unwrap_or(0) as u8)
    }

    /// Set stored brightness. Which slot the core report targets is not yet
    /// confirmed (Phase 2 capture); SaveRgbProfile commits `slot`.
    pub fn set_brightness(&self, brightness: u8, slot: u8) -> Result<()> {
        let mut core: pb::RgbProfileCore =
            self.cmd_pb(Command::GetRgbProfileCore, [0, 0, 0, slot])?;
        let p = core
            .payload
            .get_or_insert_with(|| pb::rgb_profile_core::Payload {
                a: Some(0xFFFF),
                b: Some(0xFFFF),
                ..Default::default()
            });
        p.brightness = Some(brightness as u32);
        self.report_pb(report::kind::RGB_PROFILE_CORE, FLAGS_RAM, &core)?;
        self.cmd(Command::SaveRgbProfile, [0, 0, 0, slot])?;
        self.refresh_rgb();
        Ok(())
    }

    // ----- keyboard settings (actuation, rapid trigger, tachyon) -----------

    /// Settings stored in profile `slot` (reads come from flash).
    pub fn keyboard_profile(&self, slot: u8) -> Result<pb::KeyboardProfile> {
        self.cmd_pb(Command::GetKeyboardProfile, [0, 0, 0, slot])
    }

    pub fn settings(&self, slot: u8) -> Result<KeyboardSettings> {
        Ok(KeyboardSettings::from_pb(&self.keyboard_profile(slot)?))
    }

    /// Apply settings live to the active profile; `persist` additionally
    /// writes them into that slot. Mirrors Wootility: an empty 0x1D report
    /// precedes every KeyboardProfile write with the same flags.
    pub fn set_settings(&self, s: &KeyboardSettings, persist: Option<u8>) -> Result<()> {
        let mut p = self.keyboard_profile(persist.unwrap_or(self.active_profile()?))?;
        s.apply(&mut p);
        let mut targets = vec![FLAGS_RAM];
        if let Some(slot) = persist {
            targets.push(flags_persist(slot));
        }
        let rt = self.rt_keys_raw(persist.unwrap_or(self.active_profile()?))?;
        for flags in targets {
            self.report_pb(report::kind::RT_KEYS, flags, &rt)?;
            self.report_pb(report::kind::KEYBOARD_PROFILE, flags, &p)?;
        }
        Ok(())
    }

    // ----- per-key actuation / rapid trigger ------------------------------

    /// Per-key actuation overrides stored in `slot`, as `(key id, mm)`.
    pub fn per_key_actuation(&self, slot: u8) -> Result<Vec<(String, f32)>> {
        let raw = self.per_key_raw(slot)?;
        Ok(raw
            .actuation
            .iter()
            .flat_map(|e| e.packed.iter().copied())
            .map(|v| {
                (
                    self.key_id_for_matrix((v & 0xff) as u8),
                    code_to_mm(v >> 16),
                )
            })
            .collect())
    }

    pub fn per_key_raw(&self, slot: u8) -> Result<pb::PerKey> {
        let r = self.cmd(Command::GetActuationProfile, [0, 0, 0, slot])?;
        if r.data().is_empty() {
            return Ok(pb::PerKey::default());
        }
        Ok(pb::PerKey::decode(r.data())?)
    }

    /// Set actuation overrides for keys (`None` removes the override) and
    /// write the table live and, with `persist`, to that slot.
    pub fn set_per_key_actuation(
        &self,
        changes: &[(String, Option<f32>)],
        persist: Option<u8>,
    ) -> Result<Vec<(String, f32)>> {
        let slot = persist.unwrap_or(self.active_profile()?);
        let mut raw = self.per_key_raw(slot)?;
        for (id, mm) in changes {
            let key = self
                .layout
                .key(id)
                .ok_or_else(|| Error::UnknownKey(id.clone()))?;
            for c in &key.cells {
                let mid = wooting_proto::rgb::matrix_id(c[0], c[1]) as u32;
                raw.actuation
                    .retain(|e| e.packed.first().map(|v| v & 0xff) != Some(mid));
                if let Some(mm) = mm {
                    raw.actuation.push(pb::per_key::Entry {
                        packed: vec![(mm_to_code(*mm) << 16) | mid],
                    });
                }
            }
        }
        let mut targets = vec![FLAGS_RAM];
        if let Some(slot) = persist {
            targets.push(flags_persist(slot));
        }
        for flags in targets {
            self.report_pb(report::kind::PER_KEY, flags, &raw)?;
        }
        self.per_key_actuation(slot)
    }

    /// Per-key rapid-trigger list (command 61; 54 returns nothing on fw 2.14).
    pub fn rt_keys_raw(&self, slot: u8) -> Result<pb::RtKeys> {
        let r = self.cmd(Command::GetRtKeys, [0, 0, 0, slot])?;
        if r.data().is_empty() {
            return Ok(pb::RtKeys::default());
        }
        Ok(pb::RtKeys::decode(r.data())?)
    }

    /// Keys with rapid trigger enabled individually (when it is not on for all keys).
    pub fn rt_keys(&self, slot: u8) -> Result<Vec<String>> {
        let raw = self.rt_keys_raw(slot)?;
        Ok(raw
            .keys
            .map(|k| {
                k.key
                    .iter()
                    .map(|m| self.key_id_for_matrix(*m as u8))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn set_rt_keys(
        &self,
        changes: &[(String, bool)],
        persist: Option<u8>,
    ) -> Result<Vec<String>> {
        let slot = persist.unwrap_or(self.active_profile()?);
        let mut raw = self.rt_keys_raw(slot)?;
        let keys = raw.keys.get_or_insert_with(Default::default);
        for (id, on) in changes {
            let key = self
                .layout
                .key(id)
                .ok_or_else(|| Error::UnknownKey(id.clone()))?;
            for c in &key.cells {
                let mid = wooting_proto::rgb::matrix_id(c[0], c[1]) as u32;
                keys.key.retain(|k| *k != mid);
                if *on {
                    keys.key.push(mid);
                }
            }
        }
        keys.key.sort_unstable();
        keys.key.dedup();
        keys.enabled = Some(1);
        let mut targets = vec![FLAGS_RAM];
        if let Some(slot) = persist {
            targets.push(flags_persist(slot));
        }
        for flags in targets {
            self.report_pb(report::kind::RT_KEYS, flags, &raw)?;
        }
        self.rt_keys(slot)
    }

    fn key_id_for_matrix(&self, id: u8) -> String {
        self.layout
            .key_at_cell(id >> 5, id & 0x1f)
            .map(|k| k.id.clone())
            .unwrap_or_else(|| format!("cell_{}_{}", id >> 5, id & 0x1f))
    }

    // ----- RGB effects ---------------------------------------------------

    pub fn rgb_core(&self, slot: u8) -> Result<pb::RgbProfileCore> {
        self.cmd_pb(Command::GetRgbProfileCore, [0, 0, 0, slot])
    }

    /// Replace the effect layers (and optionally brightness) of the active
    /// RGB profile; `persist` commits into that slot. SaveRgbProfile copies
    /// the whole RGB RAM, so the rows are re-sent from flash first.
    pub fn set_rgb_core(
        &self,
        effects: Option<pb::Effects>,
        brightness: Option<u8>,
        persist: Option<u8>,
    ) -> Result<pb::RgbProfileCore> {
        let slot = persist.unwrap_or(self.active_profile()?);
        let mut core = self.rgb_core(slot)?;
        if let Some(e) = effects {
            core.effects = Some(e);
        }
        if core.effects.is_none() {
            core.effects = Some(pb::Effects::default());
        }
        if let Some(b) = brightness {
            let p = core
                .payload
                .get_or_insert_with(|| pb::rgb_profile_core::Payload {
                    a: Some(0xFFFF),
                    b: Some(0xFFFF),
                    ..Default::default()
                });
            p.brightness = Some(b as u32);
        }
        if persist.is_some() {
            let frame = self.read_rgb_profile(slot)?;
            self.write_rgb_profile_with_core(&frame, &core, slot)?;
        } else {
            self.report_pb(report::kind::RGB_PROFILE_CORE, FLAGS_RAM, &core)?;
        }
        Ok(core)
    }

    fn write_rgb_profile_with_core(
        &self,
        frame: &Frame,
        core: &pb::RgbProfileCore,
        slot: u8,
    ) -> Result<()> {
        let rows = |range: std::ops::Range<usize>| pb::RgbRows {
            rows: range
                .map(|r| pb::rgb_rows::Row {
                    colors: frame.0[r].iter().map(|c| c.to_565() as u32).collect(),
                })
                .collect(),
        };
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
        self.report_pb(report::kind::RGB_PROFILE_CORE, FLAGS_RAM, core)?;
        self.cmd(Command::SaveRgbProfile, [0, 0, 0, slot])?;
        Ok(())
    }

    // ----- device config (raw DeviceConfig report) -----------------------

    /// Raw DeviceConfig bytes: `[flags0, 0, 0, 0, x, 0, flags6]`. Byte 0 bit 4 =
    /// compatibility mode (6KRO); byte 6 bit 2 clear = gamepad mode.
    pub fn device_config(&self) -> Result<Vec<u8>> {
        Ok(self.cmd(Command::GetDeviceConfig, [0; 4])?.data().to_vec())
    }

    pub fn compat_mode(&self) -> Result<bool> {
        Ok(self
            .device_config()?
            .first()
            .map(|b| b & 0x10 != 0)
            .unwrap_or(false))
    }

    pub fn gamepad_mode(&self) -> Result<bool> {
        Ok(self
            .device_config()?
            .get(6)
            .map(|b| b & 0x04 == 0)
            .unwrap_or(false))
    }

    /// Write DeviceConfig and soft-reset the keyboard (it re-enumerates;
    /// the handle becomes invalid). Mirrors Wootility: byte 4 is written as 0.
    fn write_device_config(&self, mut cfg: Vec<u8>) -> Result<()> {
        cfg.resize(7, 0);
        cfg[4] = 0;
        let bytes = report::build_raw(
            self.t.protocol(),
            report::kind::DEVICE_CONFIG,
            FLAGS_RAM,
            &cfg,
        )
        .ok_or_else(|| Error::Other("device config too large".into()))?;
        self.t.write_report(&bytes)?;
        let _ = self.t.send(Command::SoftReset, [0; 4]);
        Ok(())
    }

    pub fn set_compat_mode(&self, on: bool) -> Result<()> {
        let mut cfg = self.device_config()?;
        cfg.resize(7, 0);
        if on {
            cfg[0] |= 0x10
        } else {
            cfg[0] &= !0x10
        }
        self.write_device_config(cfg)
    }

    /// Gamepad mode makes the keyboard re-enumerate as a different USB
    /// product (0x1320); the daemon reconnects afterwards.
    pub fn set_gamepad_mode(&self, on: bool) -> Result<()> {
        let mut cfg = self.device_config()?;
        cfg.resize(7, 0);
        if on {
            cfg[6] &= !0x04
        } else {
            cfg[6] |= 0x04
        }
        self.write_device_config(cfg)
    }

    // ----- key mapping ----------------------------------------------------

    /// Mapping of `layer` (0 = main, 1/2 = Fn layers) stored in `slot`.
    /// `None` means the slot uses the default mapping for that layer.
    pub fn mapping(&self, slot: u8, layer: u8) -> Result<Option<[[u8; COLS]; ROWS]>> {
        let r = self.cmd(Command::GetMappingProfile, [0, layer, 0, slot])?;
        if r.data().is_empty() {
            return Ok(None);
        }
        let m = pb::MappingProfile::decode(r.data())?;
        Ok(Some(mapping_from_pb(&m)))
    }

    /// Mapping of `layer` with defaults filled in where the slot has none.
    pub fn mapping_or_default(&self, slot: u8, layer: u8) -> Result<[[u8; COLS]; ROWS]> {
        Ok(self
            .mapping(slot, layer)?
            .unwrap_or_else(|| self.layout.default_mapping(layer)))
    }

    /// Write a whole layer. Applies live and, with `persist`, to that slot.
    pub fn set_mapping(
        &self,
        layer: u8,
        rows: &[[u8; COLS]; ROWS],
        persist: Option<u8>,
    ) -> Result<()> {
        let m = pb::MappingProfile {
            rows: rows
                .iter()
                .map(|r| pb::mapping_profile::Row { codes: r.to_vec() })
                .collect(),
        };
        let payload = m.encode_to_vec();
        let mut targets = vec![FLAGS_RAM];
        if let Some(slot) = persist {
            targets.push(flags_persist(slot));
        }
        for flags in targets {
            let bytes = report::build_mapping(self.t.protocol(), flags, layer, &payload)
                .ok_or_else(|| Error::Other("mapping too large".into()))?;
            self.t.write_report(&bytes)?;
        }
        Ok(())
    }

    /// Remap every cell of `key_id` on `layer` to `code`.
    pub fn set_key_mapping(
        &self,
        layer: u8,
        key_id: &str,
        code: u8,
        persist: Option<u8>,
    ) -> Result<[[u8; COLS]; ROWS]> {
        let key = self
            .layout
            .key(key_id)
            .ok_or_else(|| Error::UnknownKey(key_id.to_string()))?;
        let slot = self.active_profile()?;
        let mut rows = self.mapping_or_default(slot, layer)?;
        for c in &key.cells {
            rows[c[0] as usize][c[1] as usize] = code;
        }
        self.set_mapping(layer, &rows, persist)?;
        Ok(rows)
    }

    /// Mapping as `{key_id: code_name}` for the API.
    pub fn mapping_by_key(&self, rows: &[[u8; COLS]; ROWS]) -> Vec<(String, u8, String)> {
        self.layout
            .keys
            .iter()
            .map(|k| {
                let c = k.cells[0];
                let code = rows[c[0] as usize][c[1] as usize];
                (k.id.clone(), code, keycode::name(code))
            })
            .collect()
    }

    // ----- other settings -------------------------------------------------

    pub fn global_settings(&self) -> Result<pb::GlobalSettings> {
        self.cmd_pb(Command::GetGlobalSettings, [0; 4])
    }

    pub fn set_global_settings(&self, g: &pb::GlobalSettings) -> Result<()> {
        self.report_pb(report::kind::GLOBAL_SETTINGS, FLAGS_GLOBAL, g)
    }

    pub fn advanced_keys_raw(&self, slot: u8) -> Result<pb::AdvancedKeys> {
        self.cmd_pb(Command::GetAkcProfile, [0, 0, 0, slot])
    }

    /// Advanced-key entries of `slot` resolved to key ids.
    pub fn advanced_keys(&self, slot: u8) -> Result<Vec<AdvancedKey>> {
        let raw = self.advanced_keys_raw(slot)?;
        Ok(raw
            .entries
            .iter()
            .map(|e| {
                let id = e.key.unwrap_or(0) as u8;
                let key = self
                    .layout
                    .key_at_cell(id >> 5, id & 0x1f)
                    .map(|k| k.id.clone())
                    .unwrap_or_else(|| format!("cell_{}_{}", id >> 5, id & 0x1f));
                let name = |c: Option<u32>| c.map(|c| keycode::name(c as u8));
                let other = |m: Option<u32>| m.map(|m| self.key_id_for_matrix(m as u8));
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
            .collect())
    }

    /// Replace the advanced-key table of the active profile and commit it.
    fn write_advanced_keys(&self, mut raw: pb::AdvancedKeys, slot: u8) -> Result<()> {
        raw.f2.get_or_insert(0);
        raw.count = Some(raw.entries.len() as u32);
        self.report_pb(report::kind::ADVANCED_KEYS, FLAGS_RAM, &raw)?;
        self.cmd(Command::SaveAkcProfile, [0, 0, 0, slot])?;
        Ok(())
    }

    fn matrix_of(&self, key_id: &str) -> Result<u32> {
        let key = self
            .layout
            .key(key_id)
            .ok_or_else(|| Error::UnknownKey(key_id.to_string()))?;
        let cell = key.cells[0];
        Ok(wooting_proto::rgb::matrix_id(cell[0], cell[1]) as u32)
    }

    /// Add or replace an advanced-key entry (built by the `akc_*` helpers).
    pub fn set_advanced_key(
        &self,
        key_id: &str,
        mut entry: pb::advanced_keys::Entry,
    ) -> Result<Vec<AdvancedKey>> {
        let id = self.matrix_of(key_id)?;
        entry.key = Some(id);
        entry.f9.get_or_insert(0);
        let slot = self.active_profile()?;
        let mut raw = self.advanced_keys_raw(slot)?;
        if raw.entries.len() >= 40 && !raw.entries.iter().any(|e| e.key == Some(id)) {
            return Err(Error::Other(
                "the keyboard holds at most 40 advanced keys".into(),
            ));
        }
        // Replace in place to keep the table order; append when new.
        match raw.entries.iter_mut().find(|e| e.key == Some(id)) {
            Some(e) => *e = entry,
            None => raw.entries.push(entry),
        }
        self.write_advanced_keys(raw, slot)?;
        self.advanced_keys(slot)
    }

    pub fn akc_mod_tap(tap: u8, hold: u8) -> pb::advanced_keys::Entry {
        pb::advanced_keys::Entry {
            mod_tap: Some(pb::advanced_keys::ModTap {
                tap: Some(tap as u32),
                hold: Some(hold as u32),
            }),
            ..Default::default()
        }
    }

    pub fn akc_toggle(code: u8) -> pb::advanced_keys::Entry {
        pb::advanced_keys::Entry {
            toggle: Some(pb::advanced_keys::Toggle {
                code: Some(code as u32),
            }),
            ..Default::default()
        }
    }

    pub fn akc_rappy_snappy(&self, other_key: &str) -> Result<pb::advanced_keys::Entry> {
        Ok(pb::advanced_keys::Entry {
            rappy_snappy: Some(pb::advanced_keys::RappySnappy {
                other_key: Some(self.matrix_of(other_key)?),
                p2: Some(0),
            }),
            ..Default::default()
        })
    }

    pub fn akc_socd(&self, other_key: &str, mode: u32) -> Result<pb::advanced_keys::Entry> {
        Ok(pb::advanced_keys::Entry {
            socd: Some(pb::advanced_keys::Socd {
                other_key: Some(self.matrix_of(other_key)?),
                p2: Some(0),
                mode: Some(mode),
            }),
            ..Default::default()
        })
    }

    /// DKS with four bindings (0xff = none) and the stock press-phase masks.
    pub fn akc_dks(codes: [u8; 4], masks: Option<[u8; 4]>) -> pb::advanced_keys::Entry {
        pb::advanced_keys::Entry {
            dks: Some(pb::advanced_keys::Dks {
                codes: codes.to_vec(),
                masks: masks.unwrap_or([0xa9, 0xa4, 0xa8, 0xaa]).to_vec(),
            }),
            ..Default::default()
        }
    }

    /// Add or replace a mod-tap on `key_id`: `hold` while held, `tap` when tapped.
    pub fn set_mod_tap(&self, key_id: &str, tap: u8, hold: u8) -> Result<Vec<AdvancedKey>> {
        self.set_advanced_key(key_id, Self::akc_mod_tap(tap, hold))
    }

    /// Remove any advanced-key entry on `key_id`.
    pub fn remove_advanced_key(&self, key_id: &str) -> Result<Vec<AdvancedKey>> {
        let key = self
            .layout
            .key(key_id)
            .ok_or_else(|| Error::UnknownKey(key_id.to_string()))?;
        let cell = key.cells[0];
        let id = wooting_proto::rgb::matrix_id(cell[0], cell[1]) as u32;
        let slot = self.active_profile()?;
        let mut raw = self.advanced_keys_raw(slot)?;
        raw.entries.retain(|e| e.key != Some(id));
        self.write_advanced_keys(raw, slot)?;
        self.advanced_keys(slot)
    }

    pub fn analog_values(&self) -> Result<pb::AnalogValues> {
        self.cmd_pb(Command::GetAnalogValues, [0; 4])
    }

    /// Rename a profile (and recolour its badge) in flash and, if active, in RAM.
    pub fn set_profile_metadata(
        &self,
        slot: u8,
        name: Option<&str>,
        color: Option<Rgb>,
    ) -> Result<pb::ProfileMetadata> {
        let mut m = self
            .profile_metadata(slot)?
            .ok_or(Error::NoSuchProfile(slot))?;
        if let Some(n) = name {
            m.name = n.to_string();
        }
        if let Some(c) = color {
            m.color = Some(pb::profile_metadata::Color {
                r: Some(c.r as u32),
                g: Some(c.g as u32),
                b: Some(c.b as u32),
            });
        }
        self.report_pb(report::kind::PROFILE_METADATA, flags_persist(slot), &m)?;
        if self.active_profile()? == slot {
            self.report_pb(report::kind::PROFILE_METADATA, FLAGS_RAM, &m)?;
        }
        Ok(m)
    }

    /// Delete an onboard profile slot (Wootility's command 63).
    pub fn delete_profile(&self, slot: u8) -> Result<()> {
        if self.profile_metadata(slot)?.is_none() {
            return Err(Error::NoSuchProfile(slot));
        }
        self.cmd(Command::DeleteProfile, [0, 0, 0, slot])?;
        Ok(())
    }
}

pub fn mapping_from_pb(m: &pb::MappingProfile) -> [[u8; COLS]; ROWS] {
    let mut rows = [[keycode::NONE; COLS]; ROWS];
    for (r, row) in m.rows.iter().take(ROWS).enumerate() {
        for (c, code) in row.codes.iter().take(COLS).enumerate() {
            rows[r][c] = *code;
        }
    }
    rows
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_profile_encodes_like_wootility() {
        // Omarchy profile as pushed by Wootility v5.4: 1.6 mm, RT off, 0.40 mm.
        let s = KeyboardSettings {
            actuation_mm: 1.6,
            rapid_trigger: false,
            rt_press_mm: 0.4,
            rt_release_mm: None,
            continuous_rt: false,
            tachyon: false,
        };
        let mut p = pb::KeyboardProfile::default();
        s.apply(&mut p);
        let want = [
            0x0a, 0x14, 0x08, 0x99, 0xb3, 0x01, 0x10, 0x01, 0x18, 0x00, 0x20, 0xe6, 0x8c, 0x01,
            0x28, 0x00, 0x30, 0x02, 0x38, 0x01, 0x48, 0x00,
        ];
        assert_eq!(p.encode_to_vec(), want);
        // Split sensitivity adds field 8.
        let s2 = KeyboardSettings {
            rapid_trigger: true,
            rt_press_mm: 1.0,
            rt_release_mm: Some(0.55),
            ..s
        };
        s2.apply(&mut p);
        let want2 = [
            0x0a, 0x18, 0x08, 0x99, 0xb3, 0x01, 0x10, 0x01, 0x18, 0x01, 0x20, 0x80, 0xa0, 0x01,
            0x28, 0x00, 0x30, 0x02, 0x38, 0x01, 0x40, 0xcd, 0x91, 0x01, 0x48, 0x00,
        ];
        assert_eq!(p.encode_to_vec(), want2);
        assert_eq!(code_to_mm(0x4F8F), 0.97);
    }

    #[test]
    fn advanced_keys_encode_like_wootility() {
        // RShift mod-tap: tap Up (0x4f), hold RShift (0x68), matrix id 141.
        let raw = pb::AdvancedKeys {
            entries: vec![pb::advanced_keys::Entry {
                mod_tap: Some(pb::advanced_keys::ModTap {
                    tap: Some(0x4f),
                    hold: Some(0x68),
                }),
                key: Some(141),
                f9: Some(0),
                ..Default::default()
            }],
            f2: Some(0),
            count: Some(1),
        };
        assert_eq!(
            raw.encode_to_vec(),
            [
                0x0a, 0x0b, 0x12, 0x04, 0x08, 0x4f, 0x10, 0x68, 0x40, 0x8d, 0x01, 0x48, 0x00, 0x10,
                0x00, 0x18, 0x01
            ]
        );
    }

    #[test]
    fn capture_fixtures_encode() {
        // Per-key actuation: A at 2.5 mm → d1 da 15 00 08 00 22 06 08 e1 80 fc bf 06
        let pk = pb::PerKey {
            actuation: vec![pb::per_key::Entry {
                packed: vec![(0x67FF << 16) | 0x61],
            }],
            ..Default::default()
        };
        assert_eq!(
            pk.encode_to_vec(),
            [0x22, 0x06, 0x08, 0xe1, 0x80, 0xfc, 0xbf, 0x06]
        );
        // RT keys: S enabled → 0a 04 08 62 18 01
        let rt = pb::RtKeys {
            keys: Some(pb::rt_keys::Keys {
                key: vec![0x62],
                enabled: Some(1),
            }),
        };
        assert_eq!(rt.encode_to_vec(), [0x0a, 0x04, 0x08, 0x62, 0x18, 0x01]);
        // Wave base effect, speed 8 → 22 0a 0a 08 0a 06 08 00 10 20 18 00
        let fx = pb::Effects {
            layers: vec![pb::EffectLayer {
                wave: Some(pb::effect_layer::Wave {
                    look: Some(0),
                    speed: Some(32),
                    direction: Some(0),
                }),
                ..Default::default()
            }],
        };
        let core = pb::RgbProfileCore {
            payload: Some(pb::rgb_profile_core::Payload {
                brightness: Some(255),
                a: Some(0xFFFF),
                b: Some(0xFFFF),
                c: None,
                d: None,
            }),
            effects: Some(fx),
        };
        assert_eq!(
            core.encode_to_vec(),
            [
                0x0a, 0x0b, 0x08, 0xff, 0x01, 0x10, 0xff, 0xff, 0x03, 0x18, 0xff, 0xff, 0x03, 0x22,
                0x0a, 0x0a, 0x08, 0x0a, 0x06, 0x08, 0x00, 0x10, 0x20, 0x18, 0x00
            ]
        );
        // Advanced keys: rappy snappy A/D, SOCD Tab/L-Alt, DKS W, toggle Caps→Fn2
        let akc = pb::AdvancedKeys {
            entries: vec![
                pb::advanced_keys::Entry {
                    rappy_snappy: Some(pb::advanced_keys::RappySnappy {
                        other_key: Some(0x63),
                        p2: Some(0),
                    }),
                    key: Some(0x61),
                    f9: Some(0),
                    ..Default::default()
                },
                pb::advanced_keys::Entry {
                    socd: Some(pb::advanced_keys::Socd {
                        other_key: Some(0xa2),
                        p2: Some(0),
                        mode: Some(1),
                    }),
                    key: Some(0x40),
                    f9: Some(0),
                    ..Default::default()
                },
                pb::advanced_keys::Entry {
                    dks: Some(pb::advanced_keys::Dks {
                        codes: vec![0x17, 0x64, 0xff, 0xff],
                        masks: vec![0xa9, 0xa4, 0xa8, 0xaa],
                    }),
                    key: Some(0x42),
                    f9: Some(0),
                    ..Default::default()
                },
                pb::advanced_keys::Entry {
                    toggle: Some(pb::advanced_keys::Toggle { code: Some(0xea) }),
                    key: Some(0x60),
                    f9: Some(0),
                    ..Default::default()
                },
            ],
            f2: Some(0),
            count: Some(4),
        };
        let want: Vec<u8> = "0a 0a 22 04 08 63 10 00 40 61 48 00 0a 0d 2a 07 08 a2 01 10 00 18 01 40 40 48 00 0a 12 0a 0c 0a 04 17 64 ff ff 12 04 a9 a4 a8 aa 40 42 48 00 0a 09 1a 03 08 ea 01 40 60 48 00 10 00 18 04".split(' ').map(|h| u8::from_str_radix(h, 16).unwrap()).collect();
        assert_eq!(akc.encode_to_vec(), want);
    }

    #[test]
    fn global_settings_encode() {
        let g = pb::GlobalSettings {
            power_saving: Some(pb::global_settings::PowerSaving {
                dim_after_minutes: Some(5),
                off_after_minutes: Some(10),
            }),
            numlock_inverted: Some(0),
            debounce_filter: Some(0),
        };
        assert_eq!(
            g.encode_to_vec(),
            [0x0a, 0x04, 0x08, 0x05, 0x10, 0x0a, 0x10, 0x00, 0x18, 0x00]
        );
    }
}

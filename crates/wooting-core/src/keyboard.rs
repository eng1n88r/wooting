use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{thread, time::Duration};
use wooting_hid::{DeviceInfo, Transport};
use wooting_proto::{
    pb, report, status_name, Command, Frame, Layout, Message, Response, Rgb, STATUS_OK,
};

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

    /// Write a protobuf-carrying output report.
    pub fn report_pb<M: Message>(&self, kind: u8, msg: &M) -> Result<()> {
        self.report_pb_prefixed(kind, &[], msg)
    }

    /// Write a protobuf-carrying output report with extra bytes between the
    /// length field and the message.
    pub fn report_pb_prefixed<M: Message>(&self, kind: u8, prefix: &[u8], msg: &M) -> Result<()> {
        let payload = msg.encode_to_vec();
        let bytes = report::build_with_prefix(self.t.protocol(), kind, prefix, &payload)
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
            _ => d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("."),
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
        r.data().first().copied().ok_or_else(|| Error::Other("empty profile index".into()))
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
            let Some(m) = self.profile_metadata(slot)? else { break };
            let c = m.color.unwrap_or_default();
            out.push(ProfileInfo {
                slot,
                name: m.name,
                color: Rgb::new(c.r as u8, c.g as u8, c.b as u8),
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
            let key = self.layout.key(id).ok_or_else(|| Error::UnknownKey(id.clone()))?;
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
    /// Observed on 60HE+ fw 2.14: the colour-row reports carry the slot as a
    /// byte between the length field and the protobuf, and SaveRgbProfile
    /// commits that slot. Saving without first sending rows copies the
    /// active profile's colours into the slot.
    pub fn write_rgb_profile(&self, frame: &Frame, slot: u8) -> Result<()> {
        let rows = |range: std::ops::Range<usize>| pb::RgbRows {
            rows: range
                .map(|r| pb::rgb_rows::Row {
                    colors: frame.0[r].iter().map(|c| c.to_565() as u32).collect(),
                })
                .collect(),
        };
        self.report_pb_prefixed(report::kind::RGB_PROFILE_COLORS_PART1, &[slot], &rows(0..3))?;
        self.report_pb_prefixed(report::kind::RGB_PROFILE_COLORS_PART2, &[slot], &rows(3..6))?;
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
        Ok(core.payload.map(|p| p.brightness as u8).unwrap_or(0))
    }

    /// Set stored brightness. Which slot the core report targets is not yet
    /// confirmed (Phase 2 capture); SaveRgbProfile commits `slot`.
    pub fn set_brightness(&self, brightness: u8, slot: u8) -> Result<()> {
        let mut core: pb::RgbProfileCore = self.cmd_pb(Command::GetRgbProfileCore, [0, 0, 0, slot])?;
        let p = core.payload.get_or_insert_with(|| pb::rgb_profile_core::Payload {
            a: 0xFFFF,
            b: 0xFFFF,
            ..Default::default()
        });
        p.brightness = brightness as u32;
        self.report_pb(report::kind::RGB_PROFILE_CORE, &core)?;
        self.cmd(Command::SaveRgbProfile, [0, 0, 0, slot])?;
        self.refresh_rgb();
        Ok(())
    }

    // ----- settings / mapping (read side; writes come with Phase 2) ------

    pub fn global_settings(&self) -> Result<pb::GlobalSettings> {
        self.cmd_pb(Command::GetGlobalSettings, [0; 4])
    }

    pub fn mapping(&self) -> Result<pb::MappingProfile> {
        self.cmd_pb(Command::GetMappingProfile, [0; 4])
    }

    pub fn analog_values(&self) -> Result<pb::AnalogValues> {
        self.cmd_pb(Command::GetAnalogValues, [0; 4])
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

use crate::{discover::enumerate_with, DeviceInfo, Error, Result};
use hidapi::{HidApi, HidDevice};
use std::time::Duration;
use wooting_proto::{Command, CommandPacket, Protocol, Response};

const READ_TIMEOUT_MS: i32 = 1000;
const READ_RETRIES: usize = 8;

/// An open configuration interface. One command at a time; callers that
/// share a `Transport` across threads must serialise access themselves.
pub struct Transport {
    dev: HidDevice,
    info: DeviceInfo,
}

impl Transport {
    /// Open the first Wooting keyboard found.
    pub fn open_first() -> Result<Transport> {
        let api = HidApi::new()?;
        let info = enumerate_with(&api)?
            .into_iter()
            .next()
            .ok_or(Error::NotFound)?;
        Self::open_with(&api, info)
    }

    /// Open the keyboard whose hidraw path or serial matches `selector`.
    pub fn open_matching(selector: &str) -> Result<Transport> {
        let api = HidApi::new()?;
        let info = enumerate_with(&api)?
            .into_iter()
            .find(|d| d.path == selector || d.serial.as_deref() == Some(selector))
            .ok_or(Error::NotFound)?;
        Self::open_with(&api, info)
    }

    pub fn open(info: DeviceInfo) -> Result<Transport> {
        let api = HidApi::new()?;
        Self::open_with(&api, info)
    }

    fn open_with(api: &HidApi, info: DeviceInfo) -> Result<Transport> {
        let dev = api.open_path(&std::ffi::CString::new(info.path.clone()).unwrap())?;
        dev.set_blocking_mode(true)?;
        Ok(Transport { dev, info })
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub fn protocol(&self) -> Protocol {
        self.info.protocol
    }

    /// Discard any input reports the keyboard already queued.
    fn drain(&self) -> Result<()> {
        let mut buf = vec![0u8; self.info.protocol.response_len()];
        loop {
            match self.dev.read_timeout(&mut buf, 0)? {
                0 => return Ok(()),
                n => tracing::trace!(n, "drained stale input report"),
            }
        }
    }

    /// Send a command without waiting for a reply.
    pub fn send(&self, cmd: Command, params: [u8; 4]) -> Result<()> {
        let pkt = CommandPacket::new(self.info.protocol, cmd, params);
        tracing::debug!(%cmd, bytes = ?pkt.bytes(), "feature report");
        self.dev.send_feature_report(pkt.bytes())?;
        Ok(())
    }

    /// Send a command and wait for the matching response.
    pub fn command(&self, cmd: Command, params: [u8; 4]) -> Result<Response> {
        self.drain()?;
        self.send(cmd, params)?;
        let mut buf = vec![0u8; self.info.protocol.response_len()];
        for _ in 0..READ_RETRIES {
            let n = self.dev.read_timeout(&mut buf, READ_TIMEOUT_MS)?;
            if n == 0 {
                continue;
            }
            let raw = buf[..n].to_vec();
            if Response::is_for(&raw, cmd) {
                tracing::debug!(%cmd, status = raw[4], "response");
                return Ok(Response::parse(raw, cmd)?);
            }
            tracing::trace!(?raw, "skipping unrelated input report");
        }
        Err(Error::Timeout(
            cmd,
            (READ_TIMEOUT_MS as u64) * READ_RETRIES as u64,
        ))
    }

    /// Write a raw output report (first byte is the report id).
    pub fn write_report(&self, report: &[u8]) -> Result<usize> {
        tracing::trace!(len = report.len(), id = report[0], "output report");
        Ok(self.dev.write(report)?)
    }

    /// Read one input report, if any arrives within `timeout`.
    pub fn read_report(&self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let mut buf = vec![0u8; self.info.protocol.response_len()];
        let n = self
            .dev
            .read_timeout(&mut buf, timeout.as_millis() as i32)?;
        Ok((n > 0).then(|| buf[..n].to_vec()))
    }
}

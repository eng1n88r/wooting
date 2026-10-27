use crate::{Error, Result};
use hidapi::HidApi;
use serde::Serialize;
use wooting_proto::{Model, Protocol, WOOTING_VID, WOOTING_VID_LEGACY};

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    /// hidapi device path (e.g. `/dev/hidraw8`).
    pub path: String,
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
    pub product: Option<String>,
    pub usage_page: u16,
    pub protocol: Protocol,
    pub model: Option<&'static Model>,
}

impl DeviceInfo {
    pub fn model_name(&self) -> &str {
        self.model
            .map(|m| m.name)
            .or(self.product.as_deref())
            .unwrap_or("unknown Wooting device")
    }
}

/// All Wooting configuration interfaces currently attached.
pub fn enumerate() -> Result<Vec<DeviceInfo>> {
    let api = HidApi::new()?;
    enumerate_with(&api)
}

pub(crate) fn enumerate_with(api: &HidApi) -> Result<Vec<DeviceInfo>> {
    let mut out = Vec::new();
    for d in api.device_list() {
        if d.vendor_id() != WOOTING_VID && d.vendor_id() != WOOTING_VID_LEGACY {
            continue;
        }
        let Some(protocol) = Protocol::from_usage_page(d.usage_page()) else {
            continue;
        };
        out.push(DeviceInfo {
            path: d.path().to_string_lossy().into_owned(),
            vid: d.vendor_id(),
            pid: d.product_id(),
            serial: d.serial_number().map(str::to_owned),
            product: d.product_string().map(str::to_owned),
            usage_page: d.usage_page(),
            protocol,
            model: Model::lookup(d.vendor_id(), d.product_id()),
        });
    }
    if out.is_empty() {
        return Err(Error::NotFound);
    }
    Ok(out)
}

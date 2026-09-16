use sat_core::error::Error;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Telemetry {
    pub temp: f32,
    pub bat_voltage: f32,
    pub rssi: i32,
}

pub type TelemetryVec = heapless::Vec<Telemetry, 128>;

pub fn try_encode_vec<'a>(vec: &TelemetryVec, buf: &'a mut [u8]) -> Result<&'a mut [u8], Error> {
    postcard::to_slice(vec, buf).map_err(Error::from)
}

use heapless::Vec;
use sat_core::error::Error;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Telemetry {
    pub temp: f32,
    pub bat_voltage: f32,
    pub rssi: i32,
}

pub type TelemetryVec = Vec<Telemetry, 128>;

impl Telemetry {
    pub fn try_encode<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Error> {
        postcard::to_slice(&self, buf).map_err(Error::from)
    }

    pub fn try_decode(data: &[u8]) -> Result<Self, Error> {
        postcard::from_bytes(data).map_err(Error::from)
    }
}

pub fn try_encode_vec<'a>(vec: &TelemetryVec, buf: &'a mut [u8]) -> Result<&'a mut [u8], Error> {
    postcard::to_slice(vec, buf).map_err(Error::from)
}

pub fn try_decode_vec(data: &[u8]) -> Result<TelemetryVec, Error> {
    postcard::from_bytes(data).map_err(Error::from)
}

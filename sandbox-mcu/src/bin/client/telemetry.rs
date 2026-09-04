use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct Telemetry {
    pub temp: f32,
    pub pressure: f32,
    pub bat_voltage: f32,
    pub id: u32,
}

pub type TelemetryVec = heapless::Vec<Telemetry, 128>;

pub fn encode_vec<'a>(vec: &TelemetryVec, buf: &'a mut [u8]) -> &'a mut [u8] {
    postcard::to_slice(vec, buf).unwrap()
}

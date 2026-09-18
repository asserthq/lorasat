use heapless::Vec;
use serde::{Deserialize, Serialize};

pub const MAX_CLIENT_DATA: usize = 255;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct ClientData {
    pub data: Vec<u8, MAX_CLIENT_DATA>,
}
use heapless::Vec;
use serde::{Deserialize, Serialize};

pub const MAX_CLIENT_DATA: usize = 255;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct ClientData {
    pub data: Vec<u8, MAX_CLIENT_DATA>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct Telemetry {
    pub node_addr: u32,
    pub sample_id: u32,
    pub beacon_timestamp: u32,
    pub temp: f32,
    pub bat_voltage: f32,
}

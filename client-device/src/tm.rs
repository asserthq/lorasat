use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, defmt::Format)]
pub struct Telemetry {
    pub node_addr: u32,
    pub sample_id: u32,
    pub beacon_timestamp: u64,
    pub temp: f32,
    pub bat_voltage: f32,
}

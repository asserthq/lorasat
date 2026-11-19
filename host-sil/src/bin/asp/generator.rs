use sat_core::net::Address;
use serde::Serialize;

use crate::random::Random;

#[derive(Debug, Serialize)]
pub struct Telemetry {
    pub node_addr: u32,
    pub sample_id: u32,
    pub beacon_timestamp: u64,
    pub temp: f32,
    pub bat_voltage: f32,
}

pub fn generate(
    address: Address,
    sample_id: u32,
    beacon_timestamp: u64,
    random: &mut Random,
) -> Telemetry {
    Telemetry {
        node_addr: u32::from(address.get()),
        sample_id,
        beacon_timestamp,
        temp: 20.0 + (random.next_u32() % 150) as f32 / 10.0,
        bat_voltage: 3.3 + (random.next_u32() % 7) as f32 / 10.0,
    }
}

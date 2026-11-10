use embassy_time::Duration;
use sat_core::net::Address;

pub struct CdhConfig {
    pub beacon_interval: Duration,
    pub satellite_address: Address,
    pub beacon_port: u8,
}

use sat_core::comm::{Address, Endpoint};

pub struct CommConfig {
    pub sat_addr: Address,
    pub beacon_interval_sec: u16,
    pub client_beacon_destination: Endpoint,
    pub ground_beacon_destination: Endpoint,
}

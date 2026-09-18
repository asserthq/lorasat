use sat_core::comm::address::Address;

pub struct CommConfig {
    pub sat_addr: Address,
    pub beacon_interval_sec: u16,
}

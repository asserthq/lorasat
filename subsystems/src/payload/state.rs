use sat_core::net::Endpoint;

use super::BeaconConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayloadState {
    pub beacon_config: BeaconConfig,
    pub reception_enabled: bool,
    pub transmitted_beacons: u64,
    pub beacon_send_errors: u64,
    pub dropped_notifications: u64,
    pub received_datagrams: u64,
    pub stored_records: u64,
    pub rejected_records: u64,
    pub receive_errors: u64,
    pub storage_errors: u64,
    pub last_client: Option<Endpoint>,
}

impl PayloadState {
    pub const fn new(beacon_config: BeaconConfig) -> Self {
        Self {
            beacon_config,
            reception_enabled: true,
            transmitted_beacons: 0,
            beacon_send_errors: 0,
            dropped_notifications: 0,
            received_datagrams: 0,
            stored_records: 0,
            rejected_records: 0,
            receive_errors: 0,
            storage_errors: 0,
            last_client: None,
        }
    }
}

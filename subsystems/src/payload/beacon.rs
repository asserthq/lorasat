use core::num::NonZeroU16;

use sat_core::net::{Address, Endpoint};

pub const DEFAULT_BEACON_INTERVAL_SECONDS: u16 = 10;
pub(super) const BEACON_BUFFER_SIZE: usize = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeaconConfig {
    pub satellite_address: Address,
    pub destination: Endpoint,
    pub interval_seconds: NonZeroU16,
}

impl BeaconConfig {
    pub const fn new(satellite_address: Address, destination: Endpoint) -> Self {
        Self {
            satellite_address,
            destination,
            interval_seconds: NonZeroU16::new(DEFAULT_BEACON_INTERVAL_SECONDS).unwrap(),
        }
    }
}

use serde::{Deserialize, Serialize};

use crate::comm::address::Address;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, defmt::Format)]
pub struct Beacon {
    pub sat_addr: Address,
    pub interval_sec: u16,
    pub timestamp: u64,
}

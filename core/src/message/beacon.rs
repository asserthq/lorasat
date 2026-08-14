use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Beacon {
    pub sat_addr: u32,
    pub interval_sec: u16,
    pub timestamp: u32,
}

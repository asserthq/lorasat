use heapless::Vec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct SatelliteData {
    pub data: Vec<u8, 256>,
}

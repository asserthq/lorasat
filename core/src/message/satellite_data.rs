use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct SatelliteData<'a> {
    pub data: &'a [u8],
}

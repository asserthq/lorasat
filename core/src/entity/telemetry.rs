use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct Telemetry<'a> {
    #[serde(borrow)]
    pub data: &'a [u8],
}

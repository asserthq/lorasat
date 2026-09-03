use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub enum GroundCommand {
    RequestSatTelemetry,
    RequestClientData,
    ChangeBeaconInterval(u32),
    SetTime(u64),
    Ping,
}

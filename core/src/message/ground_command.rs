use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GroundCommand {
    RequestTelemetry,
    ChangeBeaconInterval(u32),
    SetTime(u64),
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, defmt::Format)]
pub enum Command {
    RequestSatTelemetry,
    RequestClientData,
    ChangeBeaconInterval(u32),
    SetTime(u64),
    Ping,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, defmt::Format)]
pub enum Response {
    Recieved,
    Executed,
    Failed,
}

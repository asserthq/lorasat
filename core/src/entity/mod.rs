pub mod beacon;
pub mod client_data;
pub mod command;
pub mod telemetry;

pub use beacon::Beacon;
pub use client_data::ClientData;
pub use command::{Command, Response};
pub use telemetry::Telemetry;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, defmt::Format)]
pub enum DataMessage<'a> {
    #[serde(borrow)]
    Telemetry(Telemetry<'a>),
    #[serde(borrow)]
    ClientData(ClientData<'a>),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, defmt::Format)]
pub enum Datagram {
    Command(Command),
    Response(Response),
}

use crate::message::{Beacon, ClientData, Command, SatelliteData};
use core::fmt::Debug;
use serde::{Deserialize, Serialize};

#[allow(async_fn_in_trait)]
pub trait AppLayer {
    type Error: Debug;

    async fn send_message(&mut self, msg: Message) -> Result<(), Self::Error>;
    async fn recv_message<'a>(&'a mut self, buf: &'a mut [u8]) -> Result<Message, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub enum Message {
    BeaconMsg(Beacon),
    ClientDataMsg(ClientData),

    GndCommandMsg(Command),
    SatDataMsg(SatelliteData),
    GroundCommandAns,
}

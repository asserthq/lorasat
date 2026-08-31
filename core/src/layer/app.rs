use crate::message::{Beacon, ClientData, GroundCommand, SatelliteData};
use core::fmt::Debug;
use serde::{Deserialize, Serialize};

#[allow(async_fn_in_trait)]
pub trait AppLayer {
    type Error: Debug;

    async fn send_message(&mut self, msg: AppMessage) -> Result<(), Self::Error>;
    async fn recv_message<'a>(&'a mut self, buf: &'a mut [u8]) -> Result<AppMessage, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AppMessage {
    BeaconMsg(Beacon),
    ClientDataMsg(ClientData),

    GndCommandMsg(GroundCommand),
    SatDataMsg(SatelliteData),
}

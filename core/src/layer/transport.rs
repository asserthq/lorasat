use core::fmt::Debug;
use heapless::Vec;
use serde::{Deserialize, Serialize};

use core::mem::size_of;

use crate::layer::link::MAX_LINK_PAYLOAD;

pub const MAX_TRANSPORT_CHUNK_PAYLOAD: usize = MAX_LINK_PAYLOAD - size_of::<PacketHeader>();
pub const MAX_TRANSPORT_MESSAGE_PAYLOAD: usize = MAX_TRANSPORT_CHUNK_PAYLOAD * 1;

#[allow(async_fn_in_trait)]
pub trait TransportLayer {
    type Error: Debug;

    async fn send_message(&mut self, msg: Packet) -> Result<(), Self::Error>;
    async fn recv_message<'a>(&'a mut self, buf: &'a mut [u8]) -> Result<Packet, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Packet {
    pub header: PacketHeader,
    pub payload: Vec<u8, MAX_TRANSPORT_MESSAGE_PAYLOAD>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketHeader {
    pub dest_addr: u32,
    //pub kind: MessageKind,
}

// #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// pub enum MessageKind {
//     Beacon,
//     ClientData,
//     GndCommand,
//     SatData,
// }

use core::fmt::Debug;
use heapless::Vec;
use serde::{Deserialize, Serialize};

use crate::layer::data_link::MAX_DATALINK_PAYLOAD;
use core::mem::size_of;

pub const MAX_TRANSPORT_CHUNK_PAYLOAD: usize = MAX_DATALINK_PAYLOAD - size_of::<TransportHeader>();
pub const MAX_TRANSPORT_MESSAGE_PAYLOAD: usize = MAX_TRANSPORT_CHUNK_PAYLOAD * 256;

#[allow(async_fn_in_trait)]
pub trait TransportLayer {
    type Error: Debug;

    async fn try_send_message(&mut self, msg: TransportMessage) -> Result<(), Self::Error>;
    async fn try_recv_message<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> Result<TransportMessage, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportMessage {
    pub header: TransportHeader,
    pub payload: Vec<u8, MAX_TRANSPORT_MESSAGE_PAYLOAD>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportHeader {
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

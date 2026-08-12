use core::fmt::Debug;
use heapless::Vec;
use serde::{Deserialize, Serialize};

pub const MAX_TRANSPORT_MESSAGE: usize = 4096;

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
    pub dest_addr: u32,
    //pub kind: MessageKind,
    pub payload: Vec<u8, MAX_TRANSPORT_MESSAGE>,
}

// #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// pub enum MessageKind {
//     Beacon,
//     ClientData,
//     GndCommand,
//     SatData,
// }

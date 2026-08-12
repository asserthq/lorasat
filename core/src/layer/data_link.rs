use heapless::Vec;
use serde::{Deserialize, Serialize};

use core::fmt::Debug;

const MAX_DATALINK_FRAME: usize = 256;

#[allow(async_fn_in_trait)]
pub trait DataLinkLayer {
    type Error: Debug;

    async fn try_send_frame(&mut self, frame: DataLinkFrame) -> Result<(), Self::Error>;
    async fn try_recv_frame(&mut self, buf: &mut [u8]) -> Result<DataLinkFrame, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataLinkFrame {
    pub kind: FrameKind,
    pub src_addr: u32,
    pub dest_addr: u32,
    pub flags: u8,
    pub data: Vec<u8, MAX_DATALINK_FRAME>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameKind {
    Direct,
    Transport,
}

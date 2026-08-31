use crate::layer::physical::MAX_PHYSICAL_PAYLOAD;
use core::mem::size_of;
use heapless::Vec;
use serde::{Deserialize, Serialize};

use core::fmt::Debug;

pub const MAX_DATALINK_PAYLOAD: usize = MAX_PHYSICAL_PAYLOAD - size_of::<FrameHeader>();

#[allow(async_fn_in_trait)]
pub trait DataLinkLayer {
    type Error: Debug;

    async fn send_frame(&mut self, frame: DataLinkFrame) -> Result<(), Self::Error>;
    async fn recv_frame(&mut self, buf: &mut [u8]) -> Result<DataLinkFrame, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataLinkFrame {
    pub header: FrameHeader,
    pub payload: Vec<u8, MAX_DATALINK_PAYLOAD>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameHeader {
    pub kind: FrameKind,
    pub src_addr: u32,
    pub dest_addr: u32,
    pub flags: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameKind {
    Direct,
    Transport,
}

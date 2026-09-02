use crate::layer::phy::MAX_PHY_PAYLOAD;
use core::mem::size_of;
use heapless::Vec;
use serde::{Deserialize, Serialize};

use core::fmt::Debug;

pub const MAX_LINK_PAYLOAD: usize = MAX_PHY_PAYLOAD - size_of::<FrameHeader>();

#[allow(async_fn_in_trait)]
pub trait LinkLayer {
    type Error: Debug;

    async fn send_frame(&mut self, frame: Frame) -> Result<(), Self::Error>;
    async fn recv_frame(&mut self, buf: &mut [u8]) -> Result<Frame, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: Vec<u8, MAX_LINK_PAYLOAD>,
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

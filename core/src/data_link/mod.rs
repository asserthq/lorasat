pub mod data;
pub mod frame;

pub use data::{Data, DataKind};
pub use frame::Frame;

use core::fmt::Debug;

#[allow(async_fn_in_trait)]
pub trait DataLinkLayer {
    type Error: Debug;

    async fn try_send_frame(&mut self, frame: Frame) -> Result<(), Self::Error>;
    async fn try_recv_frame(&mut self, buf: &mut [u8]) -> Result<Frame, Self::Error>;
}

use crate::comm::address::Address;
use core::fmt::Debug;

#[allow(async_fn_in_trait)]
pub trait LinkLayer {
    type Error: Debug + defmt::Format;

    async fn send_beacon(&mut self, beacon: &[u8]) -> Result<(), Self::Error>;
    async fn send_frame(&mut self, dst: Address, data: &[u8]) -> Result<(), Self::Error>;
    async fn recv_frame<'a>(&mut self, buf: &'a mut [u8]) -> Result<RecvFrame<'a>, Self::Error>;

    fn addr(&self) -> Address;
}

#[derive(Debug)]
pub enum RecvFrame<'a> {
    Beacon { payload: &'a [u8] },
    Data { src: Address, payload: &'a [u8] },
}

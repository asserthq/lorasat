use core::fmt::Debug;

pub const MAX_PHY_PAYLOAD: usize = 255;

#[allow(async_fn_in_trait)]
pub trait PhyLayer {
    type Error: Debug;

    async fn send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error>;
    async fn recv_bytes<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error>;
}

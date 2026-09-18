use core::fmt::Debug;

#[allow(async_fn_in_trait)]
pub trait PhyLayer {
    type Error: Debug;

    async fn send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error>;
    async fn recv_bytes(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
}

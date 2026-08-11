use core::fmt::Debug;

#[allow(async_fn_in_trait)]
pub trait PhysicalLayer {
    type Error: Debug;

    async fn try_send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error>;
    async fn try_recv_bytes(&mut self, buf: &mut [u8]) -> Result<&mut [u8], Self::Error>;
}

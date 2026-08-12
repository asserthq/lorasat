use core::fmt::Debug;

pub const MAX_PHYSICAL_PAYLOAD: usize = 255;

#[allow(async_fn_in_trait)]
pub trait PhysicalLayer {
    type Error: Debug;

    async fn try_send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error>;
    async fn try_recv_bytes<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error>;
}

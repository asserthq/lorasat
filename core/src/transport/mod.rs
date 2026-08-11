use core::fmt::Debug;

#[allow(async_fn_in_trait)]
pub trait TransportLayer {
    type Error: Debug;

    async fn try_send_message(&mut self, data: &[u8]) -> Result<(), Self::Error>;
    async fn try_recv_message<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> Result<&'a mut [u8], Self::Error>;
}

pub mod frp;
pub mod frp_error;
pub mod frp_message;
pub mod frp_session;

use core::fmt::Debug;

#[allow(async_fn_in_trait)]
pub trait TransportLayer {
    type Error: Debug;

    async fn try_send_message(&mut self, data: &[u8]) -> Result<(), Self::Error>;
    async fn try_recv_message(&mut self, buf: &mut [u8]) -> Result<&mut [u8], Self::Error>;
}

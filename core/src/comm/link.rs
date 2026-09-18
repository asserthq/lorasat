use crate::comm::address::Address;
use core::fmt::Debug;

#[allow(async_fn_in_trait)]
pub trait LinkLayer {
    type Error: Debug + defmt::Format;

    async fn send_frame(&mut self, dst: Address, frame: &[u8]) -> Result<(), Self::Error>;
    async fn recv_frame<'a>(
        &mut self,
        buf: &'a mut [u8],
    ) -> Result<(Address, &'a [u8]), Self::Error>;
}

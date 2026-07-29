/// Half-duplex transceiver abstraction.
///
/// Any physical layer implementing this trait can be swapped
/// without changing protocol code: LoRa, UDP, UART, etc.
#[allow(async_fn_in_trait)]
pub trait HalfDuplexTransceiver {
    type Error: core::fmt::Debug;

    async fn transmit(&mut self, payload: &[u8]) -> Result<usize, Self::Error>;
    async fn receive(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
}

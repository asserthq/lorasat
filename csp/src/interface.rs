/// Packet-oriented interface between CSP and a concrete carrier.
///
/// A transceiver may implement this trait directly. A future MAC layer can
/// instead own the transceiver and implement this trait without changing CSP.
#[allow(async_fn_in_trait)]
pub trait CspInterface {
    type Error;

    /// Sends one complete CSP frame.
    async fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;

    /// Receives one complete CSP frame.
    ///
    /// This future must be cancellation-safe: after it is dropped, a later
    /// `send` or `receive` must be able to reconfigure and use the interface.
    async fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error>;
}

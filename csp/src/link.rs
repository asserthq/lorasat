#[allow(async_fn_in_trait)]
pub trait Link {
    type Error;

    /// Sends one complete CSP frame.
    async fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;

    /// Receives one complete CSP frame.
    ///
    /// This future must be cancellation-safe: after it is dropped, a later
    /// `send` or `receive` must be able to reconfigure and use the link.
    async fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error>;
}

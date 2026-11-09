use core::fmt::Debug;

use super::Endpoint;

/// Metadata for one datagram copied into a caller-provided buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, defmt::Format)]
pub struct DatagramMetadata {
    source: Endpoint,
    length: usize,
}

impl DatagramMetadata {
    pub const fn new(source: Endpoint, length: usize) -> Self {
        Self { source, length }
    }

    pub const fn source(&self) -> Endpoint {
        self.source
    }

    pub const fn length(&self) -> usize {
        self.length
    }

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }
}

/// A bound, message-oriented, connectionless communication endpoint.
///
/// This is the application-facing abstraction used by subsystems. The owner of
/// the program creates and binds the concrete endpoint, then gives this handle
/// to one subsystem. Implementations may use CSP, UDP, an in-memory test queue,
/// or another datagram transport.
///
/// A successful [`Self::send_to`] means the local transport accepted the whole
/// datagram. It does not mean the peer received or processed it.
///
/// [`Self::recv_from`] preserves message boundaries and copies exactly one
/// complete datagram into `buffer`. An implementation must return an error when
/// the datagram does not fit instead of silently truncating it.
///
/// [`Self::recv_from`] must be cancellation-safe: dropping a pending receive
/// future must not consume a datagram. This allows Embassy tasks to use it in
/// `select` with timers and command channels.
#[allow(async_fn_in_trait)]
pub trait DatagramSocket {
    type Error: Debug;

    async fn send_to(&mut self, destination: Endpoint, payload: &[u8]) -> Result<(), Self::Error>;

    async fn recv_from(&mut self, buffer: &mut [u8]) -> Result<DatagramMetadata, Self::Error>;
}

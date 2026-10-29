use core::fmt::Debug;

use super::Endpoint;

/// An established reliable, message-oriented session with one peer.
///
/// Unlike byte-stream traits, each call sends or receives one complete message.
/// A successful [`Self::send`] confirms the transport's delivery criterion, not
/// execution of an application command by the peer.
///
/// [`Self::recv`] copies one complete message into the caller-provided buffer.
/// An implementation must return an error instead of silently truncating a
/// message that does not fit. It must also be cancellation-safe: dropping a
/// pending receive future must not consume a message.
#[allow(async_fn_in_trait)]
pub trait ReliableMessageSession {
    type Error: Debug;

    fn peer(&self) -> Endpoint;

    async fn send(&mut self, message: &[u8]) -> Result<(), Self::Error>;

    async fn recv(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error>;
}

/// Accepts incoming reliable message sessions on a bound local service.
///
/// [`Self::accept`] must be cancellation-safe: dropping a pending accept future
/// must not discard an incoming session.
#[allow(async_fn_in_trait)]
pub trait SessionListener {
    type Error: Debug;
    type Session: ReliableMessageSession;

    async fn accept(&mut self) -> Result<Self::Session, Self::Error>;
}

/// Opens outgoing reliable message sessions.
#[allow(async_fn_in_trait)]
pub trait SessionConnector {
    type Error: Debug;
    type Session: ReliableMessageSession;

    async fn connect(&mut self, peer: Endpoint) -> Result<Self::Session, Self::Error>;
}

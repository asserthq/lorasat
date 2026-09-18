use sat_core::comm::address::Address;
use sat_core::comm::link::LinkLayer;
use sat_core::comm::transport::{Event, TransportLayer};

use crate::proto_impl::transport::error::TransportError;
use crate::proto_impl::transport::session::SessionImpl;

pub struct TransportImpl<L: LinkLayer> {
    link: L,
}

impl<L: LinkLayer> TransportImpl<L> {
    pub fn new(link: L) -> Self {
        Self { link }
    }
}

impl<L: LinkLayer> TransportLayer for TransportImpl<L> {
    type Error = TransportError;
    type Session = SessionImpl;

    async fn send_datagram(&mut self, dst: Address, data: &[u8]) -> Result<(), Self::Error> {
        self.link
            .send_frame(dst, data)
            .await
            .map_err(|_| TransportError::Link)
    }

    async fn connect<'a>(&mut self, _dst: Address) -> Result<Self::Session, Self::Error> {
        todo!()
    }

    async fn next<'a>(
        &mut self,
        _buf: &'a mut [u8],
    ) -> Result<Event<'a, Self::Session>, Self::Error> {
        todo!()
    }
}

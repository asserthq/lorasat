use sat_core::comm::transport::Session;

use crate::proto_impl::transport::error::TransportError;

pub struct SessionImpl {}

impl SessionImpl {
    pub fn new() -> Self {
        Self {}
    }
}

impl Session for SessionImpl {
    type Error = TransportError;

    async fn send(&mut self, _data: &[u8]) -> Result<(), Self::Error> {
        todo!()
    }

    async fn recv(&mut self, _buf: &mut [u8]) -> Result<&[u8], Self::Error> {
        todo!()
    }

    fn peer_addr(&self) -> sat_core::comm::address::Address {
        todo!()
    }
}

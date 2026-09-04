use core::future::pending;
use defmt::*;
use sat_core::layer::transport::{Packet, TransportLayer};

pub struct EmptyTransport {}
#[derive(Debug)]
pub struct EmptyError {}

impl TransportLayer for EmptyTransport {
    type Error = EmptyError;

    async fn send_message(&mut self, _msg: Packet) -> Result<(), Self::Error> {
        info!("mock sent packet");
        Ok(())
    }

    async fn recv_message<'a>(&'a mut self, _buf: &'a mut [u8]) -> Result<Packet, Self::Error> {
        info!("mock infinite recieving");
        pending().await
    }
}

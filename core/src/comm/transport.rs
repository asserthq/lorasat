use crate::comm::{address::Address, endpoint::Endpoint};
use core::fmt::Debug;

#[allow(async_fn_in_trait)]
pub trait TransportLayer {
    type Error: Debug;
    type Socket: Socket<Error = Self::Error>;

    async fn send_beacon(&mut self, beacon: &[u8]) -> Result<(), Self::Error>;
    async fn send_datagram(&mut self, endpoint: Endpoint, data: &[u8]) -> Result<(), Self::Error>;
    async fn connect<'a>(&mut self, endpoint: Endpoint) -> Result<Self::Socket, Self::Error>;

    async fn next<'a>(
        &mut self,
        buf: &'a mut [u8],
    ) -> Result<TransportEvent<'a, Self::Socket>, Self::Error>;
}

#[allow(async_fn_in_trait)]
pub trait Socket {
    type Error: Debug;

    fn port(&self) -> u8;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error>;
    async fn recv<'a>(&'a mut self, buf: &'a mut [u8]) -> Result<&'a [u8], Self::Error>;
}

pub enum TransportEvent<'a, S: Socket> {
    Beacon { payload: &'a [u8] },
    Datagram { from: Address, data: &'a [u8] },
    Session(S),
}

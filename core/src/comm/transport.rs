use crate::comm::address::Address;
use core::fmt::Debug;

#[allow(async_fn_in_trait)]
pub trait TransportLayer {
    type Error: Debug;
    type Session: Session<Error = Self::Error>;

    async fn send_datagram(&mut self, dst: Address, data: &[u8]) -> Result<(), Self::Error>;
    async fn connect<'a>(&mut self, dst: Address) -> Result<Self::Session, Self::Error>;

    async fn next<'a>(
        &mut self,
        buf: &'a mut [u8],
    ) -> Result<Event<'a, Self::Session>, Self::Error>;
}

#[allow(async_fn_in_trait)]
pub trait Session {
    type Error: Debug;

    fn peer_addr(&self) -> Address;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error>;
    async fn recv<'a>(&'a mut self, buf: &'a mut [u8]) -> Result<&'a [u8], Self::Error>;
}

pub enum Event<'a, S: Session> {
    Datagram { from: Address, data: &'a [u8] },
    Session(S),
}

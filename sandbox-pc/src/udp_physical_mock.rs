use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use tokio::net::UdpSocket;

use sat_core::layer::physical::PhysicalLayer;

/// UDP-backed async transceiver for sandbox testing.
///
/// Binds a local port. `transmit()` sends to fixed remote.
/// `receive()` awaits next datagram.
#[derive(Debug)]
pub struct UdpPhysicalMock {
    socket: UdpSocket,
    remote: SocketAddr,
}

impl UdpPhysicalMock {
    pub async fn new(local: SocketAddr, remote: SocketAddr) -> io::Result<Self> {
        let socket = UdpSocket::bind(local).await?;
        Ok(Self { socket, remote })
    }

    pub async fn from_ports(local_port: u16, remote_port: u16) -> io::Result<Self> {
        let sat_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), local_port);
        let user_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), remote_port);
        Ok(Self::new(sat_addr, user_addr).await?)
    }
}

impl PhysicalLayer for UdpPhysicalMock {
    type Error = io::Error;

    async fn send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error> {
        self.socket.send_to(payload, self.remote).await?;
        Ok(())
    }

    async fn recv_bytes<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        let (len, _) = self.socket.recv_from(buf).await?;
        Ok(&mut buf[..len])
    }
}

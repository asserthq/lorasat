use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use tokio::net::UdpSocket;

use sat_core::radio::HalfDuplexTransceiver;

/// UDP-backed async transceiver for sandbox testing.
///
/// Binds a local port. `transmit()` sends to fixed remote.
/// `receive()` awaits next datagram.
pub struct UdpTransciever {
    socket: UdpSocket,
    remote: SocketAddr,
}

impl UdpTransciever {
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

impl HalfDuplexTransceiver for UdpTransciever {
    type Error = io::Error;

    async fn transmit(&mut self, payload: &[u8]) -> Result<usize, Self::Error> {
        self.socket.send_to(payload, self.remote).await
    }

    async fn receive(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let (len, _src) = self.socket.recv_from(buf).await?;
        Ok(len)
    }
}

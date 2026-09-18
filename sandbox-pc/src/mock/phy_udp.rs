use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use sat_core::comm::address::Address;
use tokio::net::UdpSocket;

use sat_core::comm::phy::PhyLayer;

const PORT_BASE: u16 = 9000;

#[derive(Debug)]
pub struct PhyMockUDP {
    socket: UdpSocket,
    remote: SocketAddr,
}

impl PhyMockUDP {
    pub async fn new(local: SocketAddr, remote: SocketAddr) -> io::Result<Self> {
        let socket = UdpSocket::bind(local).await?;
        Ok(Self { socket, remote })
    }

    pub async fn from_ports(local_port: u16, remote_port: u16) -> io::Result<Self> {
        let local_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), local_port);
        let remote_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), remote_port);
        Ok(Self::new(local_addr, remote_addr).await?)
    }

    pub async fn from_addr(local: Address, remote: Address) -> io::Result<Self> {
        Ok(Self::from_ports(PORT_BASE + (local.0 as u16), PORT_BASE + (remote.0 as u16)).await?)
    }
}

impl PhyLayer for PhyMockUDP {
    type Error = io::Error;

    async fn send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error> {
        _ = self.socket.send_to(payload, self.remote).await?;
        Ok(())
    }

    async fn recv_bytes(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let (len, _) = self.socket.recv_from(buf).await?;
        Ok(len)
    }
}

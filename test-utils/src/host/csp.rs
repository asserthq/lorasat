use std::{io, net::SocketAddr};

use sat_csp::CspInterface;
use tokio::net::UdpSocket;

const MAX_UDP_DATAGRAM_SIZE: usize = 65_535;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioBand {
    Mhz435,
    Mhz868,
}

impl RadioBand {
    const fn port_base(self) -> u16 {
        match self {
            Self::Mhz435 => 14_350,
            Self::Mhz868 => 18_680,
        }
    }
}

fn socket_address(band: RadioBand, node: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], band.port_base() + node))
}

pub struct UdpCspInterface {
    socket: UdpSocket,
    peers: Vec<SocketAddr>,
    receive_buffer: Vec<u8>,
}

impl UdpCspInterface {
    pub async fn bind(band: RadioBand, local: u16, peers: &[u16]) -> io::Result<Self> {
        let peers: Vec<_> = peers
            .iter()
            .copied()
            .map(|peer| socket_address(band, peer))
            .collect();
        Self::bind_udp(socket_address(band, local), &peers).await
    }

    pub async fn bind_udp(local: SocketAddr, peers: &[SocketAddr]) -> io::Result<Self> {
        Ok(Self {
            socket: UdpSocket::bind(local).await?,
            peers: peers.to_vec(),
            receive_buffer: vec![0; MAX_UDP_DATAGRAM_SIZE],
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn set_peers(&mut self, peers: &[SocketAddr]) {
        self.peers = peers.to_vec();
    }
}

impl CspInterface for UdpCspInterface {
    type Error = io::Error;

    async fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        for peer in &self.peers {
            self.socket.send_to(frame, peer).await?;
        }
        Ok(())
    }

    async fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error> {
        let length = self.socket.recv(&mut self.receive_buffer).await?;
        if length > frame.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "UDP frame requires {length} bytes; buffer has {}",
                    frame.len()
                ),
            ));
        }
        frame[..length].copy_from_slice(&self.receive_buffer[..length]);
        Ok(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sends_one_frame_to_every_peer() {
        let mut sender = UdpCspInterface::bind(RadioBand::Mhz868, 401, &[402, 403])
            .await
            .unwrap();
        let mut first = UdpCspInterface::bind(RadioBand::Mhz868, 402, &[])
            .await
            .unwrap();
        let mut second = UdpCspInterface::bind(RadioBand::Mhz868, 403, &[])
            .await
            .unwrap();

        sender.send(b"complete CSP frame").await.unwrap();

        let mut buffer = [0_u8; 64];
        let length = first.receive(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..length], b"complete CSP frame");
        let length = second.receive(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..length], b"complete CSP frame");
    }

    #[tokio::test]
    async fn short_buffer_discards_whole_frame_without_partial_copy() {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let local = SocketAddr::from(([127, 0, 0, 1], 0));
            let mut receiver = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
            let mut sender = UdpCspInterface::bind_udp(local, &[receiver.local_addr().unwrap()])
                .await
                .unwrap();
            sender.send(&[0xff; 512]).await.unwrap();
            let mut small = [0xa5; 4];
            let error = receiver.receive(&mut small).await.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert_eq!(small, [0xa5; 4]);

            sender.send(b"next").await.unwrap();
            let length = receiver.receive(&mut small).await.unwrap();
            assert_eq!(&small[..length], b"next");
        })
        .await
        .expect("UDP frame small-buffer test timed out");
    }

    #[tokio::test]
    async fn empty_peer_list_makes_send_a_noop() {
        let mut interface = UdpCspInterface::bind(RadioBand::Mhz435, 404, &[])
            .await
            .unwrap();
        interface.send(b"frame").await.unwrap();
    }
}

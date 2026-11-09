use std::{io, net::SocketAddr};

use sat_csp::CspInterface;
use tokio::net::UdpSocket;

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

/// UDP-backed packet interface for running the real CSP stack on std.
///
/// One UDP datagram represents one complete lower-layer frame. Sending copies
/// the frame to every configured peer, matching the point-to-multipoint behavior
/// needed by the LoRa examples. CSP addressing and filtering remain inside the
/// real `sat-csp::Runner`.
pub struct UdpCspInterface {
    socket: UdpSocket,
    peers: Vec<SocketAddr>,
}

impl UdpCspInterface {
    pub async fn bind(band: RadioBand, local: u16, peers: &[u16]) -> io::Result<Self> {
        let socket = UdpSocket::bind(socket_address(band, local)).await?;
        let peers = peers
            .iter()
            .copied()
            .map(|peer| socket_address(band, peer))
            .collect();
        Ok(Self { socket, peers })
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
        let (length, _) = self.socket.recv_from(frame).await?;
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
    async fn empty_peer_list_makes_send_a_noop() {
        let mut interface = UdpCspInterface::bind(RadioBand::Mhz435, 404, &[])
            .await
            .unwrap();
        interface.send(b"frame").await.unwrap();
    }
}

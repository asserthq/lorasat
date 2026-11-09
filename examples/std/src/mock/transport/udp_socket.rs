use sat_core::comm::{
    Address, DatagramMetadata, DatagramSocket, Endpoint, address::BROADCAST_ADDRESS,
};
use tokio::net::UdpSocket;

use super::{MockCspError, address::endpoint_from_socket, address::socket_address};

const MAX_DATAGRAM_SIZE: usize = 4_096;

/// UDP implementation of the application-facing datagram trait.
///
/// This bypasses CSP completely. It is useful for testing subsystem logic with
/// real std sockets while preserving endpoint metadata and message boundaries.
pub struct UdpDatagramSocket {
    socket: UdpSocket,
    broadcast_peers: Vec<Address>,
    receive_buffer: Vec<u8>,
    pending: Option<(Endpoint, Vec<u8>)>,
}

impl UdpDatagramSocket {
    pub async fn bind(local: Endpoint, broadcast_peers: &[Address]) -> Result<Self, MockCspError> {
        let socket = UdpSocket::bind(socket_address(local)?).await?;
        Ok(Self {
            socket,
            broadcast_peers: broadcast_peers.to_vec(),
            receive_buffer: vec![0; MAX_DATAGRAM_SIZE],
            pending: None,
        })
    }
}

impl DatagramSocket for UdpDatagramSocket {
    type Error = MockCspError;

    async fn send_to(&mut self, destination: Endpoint, payload: &[u8]) -> Result<(), Self::Error> {
        if payload.len() > MAX_DATAGRAM_SIZE {
            return Err(MockCspError::MessageTooLarge {
                length: payload.len(),
                maximum: MAX_DATAGRAM_SIZE,
            });
        }

        if destination.addr() == BROADCAST_ADDRESS {
            for peer in &self.broadcast_peers {
                let endpoint = Endpoint::new(*peer, destination.port());
                self.socket
                    .send_to(payload, socket_address(endpoint)?)
                    .await?;
            }
        } else {
            self.socket
                .send_to(payload, socket_address(destination)?)
                .await?;
        }
        Ok(())
    }

    async fn recv_from(&mut self, buffer: &mut [u8]) -> Result<DatagramMetadata, Self::Error> {
        let (source, payload) = match self.pending.take() {
            Some(pending) => pending,
            None => {
                let (length, source) = self.socket.recv_from(&mut self.receive_buffer).await?;
                (
                    endpoint_from_socket(source)?,
                    self.receive_buffer[..length].to_vec(),
                )
            }
        };

        if payload.len() > buffer.len() {
            let required = payload.len();
            self.pending = Some((source, payload));
            return Err(MockCspError::BufferTooSmall {
                required,
                available: buffer.len(),
            });
        }

        buffer[..payload.len()].copy_from_slice(&payload);
        Ok(DatagramMetadata::new(source, payload.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip_preserves_source_endpoint() {
        let satellite_endpoint = Endpoint::new(Address(501), 11);
        let ground_endpoint = Endpoint::new(Address(502), 20);
        let mut satellite = UdpDatagramSocket::bind(satellite_endpoint, &[])
            .await
            .unwrap();
        let mut ground = UdpDatagramSocket::bind(ground_endpoint, &[]).await.unwrap();

        satellite.send_to(ground_endpoint, b"ping").await.unwrap();

        let mut buffer = [0_u8; 32];
        let metadata = ground.recv_from(&mut buffer).await.unwrap();
        assert_eq!(metadata.source(), satellite_endpoint);
        assert_eq!(&buffer[..metadata.length()], b"ping");
    }

    #[tokio::test]
    async fn buffer_error_keeps_complete_message() {
        let sender_endpoint = Endpoint::new(Address(503), 11);
        let receiver_endpoint = Endpoint::new(Address(504), 11);
        let mut sender = UdpDatagramSocket::bind(sender_endpoint, &[]).await.unwrap();
        let mut receiver = UdpDatagramSocket::bind(receiver_endpoint, &[])
            .await
            .unwrap();

        sender
            .send_to(receiver_endpoint, b"message larger than buffer")
            .await
            .unwrap();

        let mut small = [0_u8; 4];
        assert!(matches!(
            receiver.recv_from(&mut small).await,
            Err(MockCspError::BufferTooSmall { .. })
        ));
        let mut large = [0_u8; 64];
        let metadata = receiver.recv_from(&mut large).await.unwrap();
        assert_eq!(&large[..metadata.length()], b"message larger than buffer");
    }

    #[tokio::test]
    async fn broadcast_reaches_configured_peers() {
        let sender_endpoint = Endpoint::new(Address(505), 11);
        let first_endpoint = Endpoint::new(Address(506), 10);
        let second_endpoint = Endpoint::new(Address(507), 10);
        let mut sender = UdpDatagramSocket::bind(sender_endpoint, &[Address(506), Address(507)])
            .await
            .unwrap();
        let mut first = UdpDatagramSocket::bind(first_endpoint, &[]).await.unwrap();
        let mut second = UdpDatagramSocket::bind(second_endpoint, &[]).await.unwrap();

        sender
            .send_to(Endpoint::new(BROADCAST_ADDRESS, 10), b"beacon")
            .await
            .unwrap();

        let mut buffer = [0_u8; 16];
        let first_metadata = first.recv_from(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..first_metadata.length()], b"beacon");
        let second_metadata = second.recv_from(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..second_metadata.length()], b"beacon");
    }
}

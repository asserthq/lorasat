use sat_core::net::{Address, DatagramMetadata, DatagramSocket, Endpoint};
use tokio::net::UdpSocket;

use super::{MockNetError, address::endpoint_from_socket, address::socket_address};

const MAX_DATAGRAM_SIZE: usize = 4_096;

pub struct UdpDatagramSocket {
    socket: UdpSocket,
    broadcast_peers: Vec<Address>,
    receive_buffer: Vec<u8>,
    pending: Option<(Endpoint, Vec<u8>)>,
}

impl UdpDatagramSocket {
    pub async fn bind(local: Endpoint, broadcast_peers: &[Address]) -> Result<Self, MockNetError> {
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
    type Error = MockNetError;

    fn discard_oversized(&mut self, capacity: usize) -> Option<DatagramMetadata> {
        let (source, payload) = self.pending.as_ref()?;
        if payload.len() <= capacity {
            return None;
        }
        let metadata = DatagramMetadata::new(*source, payload.len());
        self.pending = None;
        Some(metadata)
    }

    async fn send_to(&mut self, destination: Endpoint, payload: &[u8]) -> Result<(), Self::Error> {
        if payload.len() > MAX_DATAGRAM_SIZE {
            return Err(MockNetError::MessageTooLarge {
                length: payload.len(),
                maximum: MAX_DATAGRAM_SIZE,
            });
        }

        if destination.addr().is_broadcast() {
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
            return Err(MockNetError::BufferTooSmall {
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
        let satellite_endpoint = Endpoint::new(Address::new_unicast(501).unwrap(), 11);
        let ground_endpoint = Endpoint::new(Address::new_unicast(502).unwrap(), 20);
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
        let sender_endpoint = Endpoint::new(Address::new_unicast(503).unwrap(), 11);
        let receiver_endpoint = Endpoint::new(Address::new_unicast(504).unwrap(), 11);
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
            Err(MockNetError::BufferTooSmall { .. })
        ));
        let mut large = [0_u8; 64];
        let metadata = receiver.recv_from(&mut large).await.unwrap();
        assert_eq!(&large[..metadata.length()], b"message larger than buffer");
    }

    #[tokio::test]
    async fn explicit_discard_drops_only_oversized_pending_datagrams() {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let source = Endpoint::new(Address::new_unicast(777).unwrap(), 11);
            let mut receiver = UdpDatagramSocket {
                socket: UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                    .await
                    .unwrap(),
                broadcast_peers: Vec::new(),
                receive_buffer: vec![0; MAX_DATAGRAM_SIZE],
                pending: Some((source, vec![0; 257])),
            };
            assert_eq!(receiver.discard_oversized(257), None);
            assert!(receiver.pending.is_some());
            assert_eq!(
                receiver.discard_oversized(256),
                Some(DatagramMetadata::new(source, 257))
            );
            assert!(receiver.pending.is_none());
            assert_eq!(receiver.discard_oversized(256), None);
            receiver.pending = Some((source, b"valid".to_vec()));
            assert_eq!(receiver.discard_oversized(256), None);
            let mut buffer = [0; 256];
            let metadata = receiver.recv_from(&mut buffer).await.unwrap();
            assert_eq!(metadata.source(), source);
            assert_eq!(&buffer[..metadata.length()], b"valid");
        })
        .await
        .expect("pending datagram discard test timed out");
    }

    #[tokio::test]
    async fn broadcast_reaches_configured_peers() {
        let sender_endpoint = Endpoint::new(Address::new_unicast(505).unwrap(), 11);
        let first_endpoint = Endpoint::new(Address::new_unicast(506).unwrap(), 10);
        let second_endpoint = Endpoint::new(Address::new_unicast(507).unwrap(), 10);
        let mut sender = UdpDatagramSocket::bind(
            sender_endpoint,
            &[
                Address::new_unicast(506).unwrap(),
                Address::new_unicast(507).unwrap(),
            ],
        )
        .await
        .unwrap();
        let mut first = UdpDatagramSocket::bind(first_endpoint, &[]).await.unwrap();
        let mut second = UdpDatagramSocket::bind(second_endpoint, &[]).await.unwrap();

        sender
            .send_to(Endpoint::broadcast(10), b"beacon")
            .await
            .unwrap();

        let mut buffer = [0_u8; 16];
        let first_metadata = first.recv_from(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..first_metadata.length()], b"beacon");
        let second_metadata = second.recv_from(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..second_metadata.length()], b"beacon");
    }
}

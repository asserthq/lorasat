use sat_core::net::{
    Address, DatagramMetadata, DatagramSocket, Endpoint, ReliableSession, SessionListener,
};
use sat_csp::{
    Datagram, MAX_NODE_ID, MAX_PORT, RdpConnection, RdpError, RdpListener, RdpMessage,
    RdpResources, SendError, Socket,
};
use tokio::time::{Duration, interval};

pub const MTU: usize = 255;
pub const SATELLITE_ADDRESS: u16 = 1;
pub const GROUND_ADDRESS: u16 = 2;
pub const CLIENT_ADDRESS: u16 = 3;

pub const BEACON_PORT: u8 = 10;
pub const CLIENT_DATA_PORT: u8 = 11;
pub const COMMAND_PORT: u8 = 20;

pub const BEACON_INTERVAL_SECONDS: u16 = 10;
pub const DUMP_COMMAND: &[u8] = b"dump";

const BEACON_VERSION: u8 = 1;
const BEACON_SIZE: usize = 13;
const DUMP_VERSION: u8 = 1;
const RDP_APPLICATION_MAX: usize = MTU - sat_csp::HEADER_SIZE - sat_csp::RDP_TRAILER_SIZE;

#[derive(Debug)]
pub enum CspDatagramError {
    InvalidAddress(Address),
    InvalidPort(u8),
    BufferTooSmall { required: usize, available: usize },
    Send(SendError),
}

pub struct CspDatagramSocket<'a, const MTU: usize, const TX_DEPTH: usize, const RX_DEPTH: usize> {
    socket: Socket<'a, MTU, TX_DEPTH, RX_DEPTH>,
    pending: Option<Datagram<MTU>>,
}

impl<'a, const MTU: usize, const TX_DEPTH: usize, const RX_DEPTH: usize>
    CspDatagramSocket<'a, MTU, TX_DEPTH, RX_DEPTH>
{
    pub const fn new(socket: Socket<'a, MTU, TX_DEPTH, RX_DEPTH>) -> Self {
        Self {
            socket,
            pending: None,
        }
    }
}

impl<const MTU: usize, const TX_DEPTH: usize, const RX_DEPTH: usize> DatagramSocket
    for CspDatagramSocket<'_, MTU, TX_DEPTH, RX_DEPTH>
{
    type Error = CspDatagramError;

    async fn send_to(&mut self, destination: Endpoint, payload: &[u8]) -> Result<(), Self::Error> {
        let address = destination.addr().get();
        if address > MAX_NODE_ID {
            return Err(CspDatagramError::InvalidAddress(destination.addr()));
        }
        if destination.port() > MAX_PORT {
            return Err(CspDatagramError::InvalidPort(destination.port()));
        }

        self.socket
            .send_to(address, destination.port(), payload)
            .await
            .map_err(CspDatagramError::Send)
    }

    async fn recv_from(&mut self, buffer: &mut [u8]) -> Result<DatagramMetadata, Self::Error> {
        let datagram = match self.pending.take() {
            Some(datagram) => datagram,
            None => self.socket.recv_from().await,
        };
        let length = datagram.payload().len();
        if length > buffer.len() {
            self.pending = Some(datagram);
            return Err(CspDatagramError::BufferTooSmall {
                required: length,
                available: buffer.len(),
            });
        }

        buffer[..length].copy_from_slice(datagram.payload());
        Ok(DatagramMetadata::new(
            Endpoint::new(
                Address::new(datagram.source()).expect("CSP source address must be valid"),
                datagram.source_port(),
            ),
            length,
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Beacon {
    pub satellite_address: u16,
    pub unix_time_seconds: u64,
    pub interval_seconds: u16,
}

pub fn unix_time_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beacon_round_trip() {
        let beacon = Beacon {
            satellite_address: 42,
            unix_time_seconds: 123_456,
            interval_seconds: 10,
        };
        assert_eq!(Beacon::decode(&beacon.encode()), Ok(beacon));
    }

    #[test]
    fn opaque_dump_round_trip() {
        let records = vec![OpaqueRecord {
            source: 3,
            source_port: 10,
            payload: b"unknown subscriber bytes".to_vec(),
        }];
        assert_eq!(
            decode_dump(&encode_dump(&records)),
            Ok(DumpResponse {
                total_records: 1,
                records,
            })
        );
    }
}

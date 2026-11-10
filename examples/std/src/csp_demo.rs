use sat_core::net::{
    Address, DatagramMetadata, DatagramSocket, Endpoint, ReliableSession, SessionListener,
    address::BROADCAST_ADDRESS,
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
        let address = if destination.addr() == BROADCAST_ADDRESS {
            MAX_NODE_ID
        } else {
            let address = u16::try_from(destination.addr().0)
                .map_err(|_| CspDatagramError::InvalidAddress(destination.addr()))?;
            if address > MAX_NODE_ID {
                return Err(CspDatagramError::InvalidAddress(destination.addr()));
            }
            address
        };
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
                Address(u32::from(datagram.source())),
                datagram.source_port(),
            ),
            length,
        ))
    }
}

#[derive(Debug)]
pub enum CspRdpSessionError {
    BufferTooSmall { required: usize, available: usize },
    Rdp(RdpError),
}

pub struct CspRdpSession<'a, const MTU: usize> {
    connection: RdpConnection<'a, MTU>,
    pending: Option<RdpMessage<MTU>>,
}

impl<const MTU: usize> ReliableSession for CspRdpSession<'_, MTU> {
    type Error = CspRdpSessionError;

    fn peer(&self) -> Endpoint {
        Endpoint::new(
            Address(u32::from(self.connection.peer_address())),
            self.connection.peer_port(),
        )
    }

    async fn send(&mut self, message: &[u8]) -> Result<(), Self::Error> {
        self.connection
            .send(message)
            .await
            .map_err(CspRdpSessionError::Rdp)
    }

    async fn recv(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        let message = match self.pending.take() {
            Some(message) => message,
            None => self.connection.recv().await,
        };
        let length = message.payload().len();
        if length > buffer.len() {
            self.pending = Some(message);
            return Err(CspRdpSessionError::BufferTooSmall {
                required: length,
                available: buffer.len(),
            });
        }

        buffer[..length].copy_from_slice(message.payload());
        Ok(length)
    }
}

pub struct CspRdpListener<'a, const MTU: usize> {
    listener: RdpListener<'a, MTU>,
}

impl<'a, const MTU: usize> CspRdpListener<'a, MTU> {
    pub const fn new(listener: RdpListener<'a, MTU>) -> Self {
        Self { listener }
    }
}

impl<'a, const MTU: usize> SessionListener for CspRdpListener<'a, MTU> {
    type Error = RdpError;
    type Session = CspRdpSession<'a, MTU>;

    async fn accept(&mut self) -> Result<Self::Session, Self::Error> {
        let connection = self.listener.accept().await?;
        Ok(CspRdpSession {
            connection,
            pending: None,
        })
    }
}

pub async fn run_rdp_clock<const MTU: usize>(resources: &RdpResources<MTU>) -> ! {
    let mut now_ms = 0_u32;
    let mut ticks = interval(Duration::from_millis(10));
    loop {
        ticks.tick().await;
        now_ms = now_ms.wrapping_add(10);
        resources.update_time(now_ms);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Beacon {
    pub satellite_address: u16,
    pub unix_time_seconds: u64,
    pub interval_seconds: u16,
}

impl Beacon {
    pub fn encode(self) -> [u8; BEACON_SIZE] {
        let mut output = [0; BEACON_SIZE];
        output[0] = BEACON_VERSION;
        output[1..3].copy_from_slice(&self.satellite_address.to_be_bytes());
        output[3..11].copy_from_slice(&self.unix_time_seconds.to_be_bytes());
        output[11..13].copy_from_slice(&self.interval_seconds.to_be_bytes());
        output
    }

    pub fn decode(input: &[u8]) -> Result<Self, &'static str> {
        if input.len() != BEACON_SIZE {
            return Err("wrong beacon length");
        }
        if input[0] != BEACON_VERSION {
            return Err("unsupported beacon version");
        }
        Ok(Self {
            satellite_address: u16::from_be_bytes([input[1], input[2]]),
            unix_time_seconds: u64::from_be_bytes([
                input[3], input[4], input[5], input[6], input[7], input[8], input[9], input[10],
            ]),
            interval_seconds: u16::from_be_bytes([input[11], input[12]]),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpaqueRecord {
    pub source: u16,
    pub source_port: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct DumpResponse {
    pub total_records: usize,
    pub records: Vec<OpaqueRecord>,
}

pub fn encode_dump(records: &[OpaqueRecord]) -> Vec<u8> {
    let mut output = Vec::with_capacity(RDP_APPLICATION_MAX);
    output.extend_from_slice(&[DUMP_VERSION, 0, records.len().min(u8::MAX as usize) as u8]);

    let mut included = 0_u8;
    for record in records {
        let Ok(payload_length) = u8::try_from(record.payload.len()) else {
            continue;
        };
        let required = 4 + usize::from(payload_length);
        if output.len() + required > RDP_APPLICATION_MAX || included == u8::MAX {
            break;
        }
        output.extend_from_slice(&record.source.to_be_bytes());
        output.push(record.source_port);
        output.push(payload_length);
        output.extend_from_slice(&record.payload);
        included += 1;
    }
    output[1] = included;
    output
}

pub fn decode_dump(input: &[u8]) -> Result<DumpResponse, &'static str> {
    if input.len() < 3 {
        return Err("dump response is too short");
    }
    if input[0] != DUMP_VERSION {
        return Err("unsupported dump version");
    }

    let included = usize::from(input[1]);
    let total_records = usize::from(input[2]);
    let mut offset = 3;
    let mut records = Vec::with_capacity(included);
    for _ in 0..included {
        if input.len() < offset + 4 {
            return Err("truncated record header");
        }
        let source = u16::from_be_bytes([input[offset], input[offset + 1]]);
        let source_port = input[offset + 2];
        let payload_length = usize::from(input[offset + 3]);
        offset += 4;
        if input.len() < offset + payload_length {
            return Err("truncated record payload");
        }
        records.push(OpaqueRecord {
            source,
            source_port,
            payload: input[offset..offset + payload_length].to_vec(),
        });
        offset += payload_length;
    }
    if offset != input.len() {
        return Err("trailing dump bytes");
    }
    Ok(DumpResponse {
        total_records,
        records,
    })
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

use sat_core::net::{Address, DatagramMetadata, DatagramSocket, Endpoint};
use sat_csp::{Datagram, MAX_NODE_ID, MAX_PORT, SendError, Socket};

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
        Ok(datagram_metadata(&datagram))
    }

    fn discard_oversized(&mut self, capacity: usize) -> Option<DatagramMetadata> {
        let datagram = self.pending.as_ref()?;
        if datagram.payload().len() <= capacity {
            return None;
        }

        let metadata = datagram_metadata(datagram);
        self.pending = None;
        Some(metadata)
    }
}

fn datagram_metadata<const MTU: usize>(datagram: &Datagram<MTU>) -> DatagramMetadata {
    DatagramMetadata::new(
        Endpoint::new(
            Address::new(datagram.source()).expect("CSP source address must be valid"),
            datagram.source_port(),
        ),
        datagram.payload().len(),
    )
}

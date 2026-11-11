use sat_core::net::{
    Address, DatagramMetadata, DatagramSocket, Endpoint, ReliableSession, SessionListener,
};
use sat_csp::{
    Datagram, MAX_NODE_ID, MAX_PORT, RdpConnection, RdpError, RdpListener, RdpMessage,
    RdpResources, SendError, Socket,
};
use tokio::time::{Duration, interval};

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
            Address::new(self.connection.peer_address()).expect("CSP peer address must be valid"),
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

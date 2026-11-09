use std::io;

use sat_core::comm::{Endpoint, ReliableMessageSession, SessionConnector, SessionListener};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{
        TcpListener, TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    sync::mpsc,
    task::JoinHandle,
};

use super::{
    MockCspError,
    address::{decode_endpoint, encode_endpoint, socket_address},
};

const MAX_SESSION_MESSAGE_SIZE: usize = 65_535;
const SESSION_QUEUE_DEPTH: usize = 8;
const LISTENER_QUEUE_DEPTH: usize = 4;

/// TCP reliable message session with explicit message framing.
///
/// A background reader owns the TCP read half and places complete messages into
/// a bounded queue. Therefore cancelling `recv` cannot consume a partial frame.
pub struct TcpSession {
    peer: Endpoint,
    writer: OwnedWriteHalf,
    incoming: mpsc::Receiver<Result<Vec<u8>, io::Error>>,
    pending: Option<Vec<u8>>,
    reader_task: JoinHandle<()>,
}

impl Drop for TcpSession {
    fn drop(&mut self) {
        self.reader_task.abort();
    }
}

impl ReliableMessageSession for TcpSession {
    type Error = MockCspError;

    fn peer(&self) -> Endpoint {
        self.peer
    }

    async fn send(&mut self, message: &[u8]) -> Result<(), Self::Error> {
        if message.len() > MAX_SESSION_MESSAGE_SIZE {
            return Err(MockCspError::MessageTooLarge {
                length: message.len(),
                maximum: MAX_SESSION_MESSAGE_SIZE,
            });
        }

        self.writer
            .write_all(&(message.len() as u32).to_be_bytes())
            .await?;
        self.writer.write_all(message).await?;
        Ok(())
    }

    async fn recv(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        let message = match self.pending.take() {
            Some(message) => message,
            None => self.incoming.recv().await.ok_or(MockCspError::Closed)??,
        };

        if message.len() > buffer.len() {
            let required = message.len();
            self.pending = Some(message);
            return Err(MockCspError::BufferTooSmall {
                required,
                available: buffer.len(),
            });
        }

        buffer[..message.len()].copy_from_slice(&message);
        Ok(message.len())
    }
}

pub struct TcpSessionListener {
    incoming: mpsc::Receiver<Result<TcpSession, MockCspError>>,
    accept_task: JoinHandle<()>,
}

impl TcpSessionListener {
    pub async fn bind(local: Endpoint) -> Result<Self, MockCspError> {
        let listener = TcpListener::bind(socket_address(local)?).await?;
        let (sender, incoming) = mpsc::channel(LISTENER_QUEUE_DEPTH);
        let accept_task = tokio::spawn(async move {
            loop {
                let session = accept_session(&listener).await;
                if sender.send(session).await.is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            incoming,
            accept_task,
        })
    }
}

impl Drop for TcpSessionListener {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

impl SessionListener for TcpSessionListener {
    type Error = MockCspError;
    type Session = TcpSession;

    async fn accept(&mut self) -> Result<Self::Session, Self::Error> {
        self.incoming.recv().await.ok_or(MockCspError::Closed)?
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TcpSessionConnector {
    local: Endpoint,
}

impl TcpSessionConnector {
    pub const fn new(local: Endpoint) -> Self {
        Self { local }
    }
}

impl SessionConnector for TcpSessionConnector {
    type Error = MockCspError;
    type Session = TcpSession;

    async fn connect(&mut self, peer: Endpoint) -> Result<Self::Session, Self::Error> {
        socket_address(self.local)?;
        let mut stream = TcpStream::connect(socket_address(peer)?).await?;
        stream.write_all(&encode_endpoint(self.local)).await?;
        Ok(create_session(stream, peer))
    }
}

async fn accept_session(listener: &TcpListener) -> Result<TcpSession, MockCspError> {
    let (mut stream, _) = listener.accept().await?;
    let mut encoded_peer = [0_u8; 5];
    stream.read_exact(&mut encoded_peer).await?;
    let peer = decode_endpoint(encoded_peer);
    socket_address(peer)?;
    Ok(create_session(stream, peer))
}

fn create_session(stream: TcpStream, peer: Endpoint) -> TcpSession {
    let (reader, writer) = stream.into_split();
    let (sender, incoming) = mpsc::channel(SESSION_QUEUE_DEPTH);
    let reader_task = tokio::spawn(read_messages(reader, sender));
    TcpSession {
        peer,
        writer,
        incoming,
        pending: None,
        reader_task,
    }
}

async fn read_messages(
    mut reader: OwnedReadHalf,
    sender: mpsc::Sender<Result<Vec<u8>, io::Error>>,
) {
    loop {
        let mut encoded_length = [0_u8; 4];
        if let Err(error) = reader.read_exact(&mut encoded_length).await {
            let _ = sender.send(Err(error)).await;
            break;
        }
        let length = u32::from_be_bytes(encoded_length) as usize;
        if length > MAX_SESSION_MESSAGE_SIZE {
            let _ = sender
                .send(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "mock CSP session message exceeds configured limit",
                )))
                .await;
            break;
        }

        let mut message = vec![0_u8; length];
        if let Err(error) = reader.read_exact(&mut message).await {
            let _ = sender.send(Err(error)).await;
            break;
        }
        if sender.send(Ok(message)).await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sat_core::comm::{Address, ReliableMessageSession, SessionConnector, SessionListener};

    use super::*;

    #[tokio::test]
    async fn round_trip_preserves_message_boundaries() {
        let server_endpoint = Endpoint::new(Address(508), 20);
        let client_endpoint = Endpoint::new(Address(509), 63);
        let mut listener = TcpSessionListener::bind(server_endpoint).await.unwrap();
        let mut connector = TcpSessionConnector::new(client_endpoint);

        let (client, server) = tokio::join!(connector.connect(server_endpoint), listener.accept());
        let mut client = client.unwrap();
        let mut server = server.unwrap();
        assert_eq!(client.peer(), server_endpoint);
        assert_eq!(server.peer(), client_endpoint);

        client.send(b"command").await.unwrap();
        let mut buffer = [0_u8; 32];
        let length = server.recv(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..length], b"command");

        server.send(b"response").await.unwrap();
        let length = client.recv(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..length], b"response");
    }

    #[tokio::test]
    async fn buffer_error_keeps_complete_message() {
        let server_endpoint = Endpoint::new(Address(512), 20);
        let client_endpoint = Endpoint::new(Address(513), 63);
        let mut listener = TcpSessionListener::bind(server_endpoint).await.unwrap();
        let mut connector = TcpSessionConnector::new(client_endpoint);
        let (client, server) = tokio::join!(connector.connect(server_endpoint), listener.accept());
        let mut client = client.unwrap();
        let mut server = server.unwrap();

        client.send(b"complete message").await.unwrap();
        let mut small = [0_u8; 4];
        assert!(matches!(
            server.recv(&mut small).await,
            Err(MockCspError::BufferTooSmall { .. })
        ));
        let mut large = [0_u8; 32];
        let length = server.recv(&mut large).await.unwrap();
        assert_eq!(&large[..length], b"complete message");
    }

    #[tokio::test]
    async fn cancelled_accept_does_not_break_listener() {
        let server_endpoint = Endpoint::new(Address(510), 20);
        let client_endpoint = Endpoint::new(Address(511), 63);
        let mut listener = TcpSessionListener::bind(server_endpoint).await.unwrap();
        let result = tokio::time::timeout(Duration::from_millis(20), listener.accept()).await;
        assert!(result.is_err());

        let mut connector = TcpSessionConnector::new(client_endpoint);
        let (client, server) = tokio::join!(connector.connect(server_endpoint), listener.accept());
        assert!(client.is_ok());
        assert!(server.is_ok());
    }
}

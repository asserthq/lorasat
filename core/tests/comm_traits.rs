use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

use sat_core::comm::{Address, DatagramMetadata, DatagramSocket, Endpoint, ReliableMessageSession};

fn run_ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());

    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("test future unexpectedly pending"),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum MockError {
    BufferTooSmall,
    NoMessage,
}

struct MockDatagramSocket {
    inbound: Option<(Endpoint, &'static [u8])>,
    sent_to: Option<Endpoint>,
    sent: [u8; 16],
    sent_len: usize,
}

impl MockDatagramSocket {
    fn with_inbound(source: Endpoint, payload: &'static [u8]) -> Self {
        Self {
            inbound: Some((source, payload)),
            sent_to: None,
            sent: [0; 16],
            sent_len: 0,
        }
    }
}

impl DatagramSocket for MockDatagramSocket {
    type Error = MockError;

    async fn send_to(&mut self, destination: Endpoint, payload: &[u8]) -> Result<(), Self::Error> {
        if payload.len() > self.sent.len() {
            return Err(MockError::BufferTooSmall);
        }

        self.sent[..payload.len()].copy_from_slice(payload);
        self.sent_len = payload.len();
        self.sent_to = Some(destination);
        Ok(())
    }

    async fn recv_from(&mut self, buffer: &mut [u8]) -> Result<DatagramMetadata, Self::Error> {
        let (source, payload) = self.inbound.take().ok_or(MockError::NoMessage)?;
        if payload.len() > buffer.len() {
            self.inbound = Some((source, payload));
            return Err(MockError::BufferTooSmall);
        }

        buffer[..payload.len()].copy_from_slice(payload);
        Ok(DatagramMetadata::new(source, payload.len()))
    }
}

struct MockSession {
    peer: Endpoint,
    inbound: Option<&'static [u8]>,
}

impl ReliableMessageSession for MockSession {
    type Error = MockError;

    fn peer(&self) -> Endpoint {
        self.peer
    }

    async fn send(&mut self, _message: &[u8]) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn recv(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        let message = self.inbound.take().ok_or(MockError::NoMessage)?;
        if message.len() > buffer.len() {
            self.inbound = Some(message);
            return Err(MockError::BufferTooSmall);
        }

        buffer[..message.len()].copy_from_slice(message);
        Ok(message.len())
    }
}

#[test]
fn datagram_preserves_source_and_message_boundary() {
    let source = Endpoint::new(Address(10), 7);
    let destination = Endpoint::new(Address(20), 8);
    let mut socket = MockDatagramSocket::with_inbound(source, b"record");
    let mut buffer = [0; 16];

    let metadata = run_ready(socket.recv_from(&mut buffer)).unwrap();
    assert_eq!(metadata.source(), source);
    assert_eq!(metadata.length(), 6);
    assert_eq!(&buffer[..metadata.length()], b"record");

    run_ready(socket.send_to(destination, b"beacon")).unwrap();
    assert_eq!(socket.sent_to, Some(destination));
    assert_eq!(&socket.sent[..socket.sent_len], b"beacon");
}

#[test]
fn datagram_does_not_silently_truncate() {
    let source = Endpoint::new(Address(10), 7);
    let mut socket = MockDatagramSocket::with_inbound(source, b"too large");
    let mut small = [0; 4];

    assert_eq!(
        run_ready(socket.recv_from(&mut small)),
        Err(MockError::BufferTooSmall)
    );

    let mut enough = [0; 16];
    let metadata = run_ready(socket.recv_from(&mut enough)).unwrap();
    assert_eq!(&enough[..metadata.length()], b"too large");
}

#[test]
fn reliable_session_receives_one_complete_message() {
    let peer = Endpoint::new(Address(1), 12);
    let mut session = MockSession {
        peer,
        inbound: Some(b"command"),
    };
    let mut small = [0; 3];

    assert_eq!(session.peer(), peer);
    assert_eq!(
        run_ready(session.recv(&mut small)),
        Err(MockError::BufferTooSmall)
    );

    let mut enough = [0; 16];
    let length = run_ready(session.recv(&mut enough)).unwrap();
    assert_eq!(&enough[..length], b"command");
}

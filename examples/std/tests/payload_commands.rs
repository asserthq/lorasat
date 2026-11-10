use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    io,
    net::SocketAddr,
    rc::Rc,
    task::{Context, Waker},
};

use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel};
use examples_std::{
    csp_demo::{BEACON_PORT, CLIENT_DATA_PORT, CspDatagramSocket, MTU},
    mock::UdpCspInterface,
    payload_demo::{
        COMMAND_PORT, Message, RX_ENDPOINT, RX_NODE, TX_ENDPOINT, TX_NODE, send_ping,
        serve_commands,
    },
};
use sat_core::net::{DatagramMetadata, DatagramSocket, Endpoint};
use sat_csp::{RunnerBuilder, StackResources};
use sat_drivers::storage::RamStorage;
use sat_subsystems::payload::{PayloadCommand, PayloadEvent, PayloadSystem};
use tokio::time::{Duration, timeout};

type Datagram = (Endpoint, Vec<u8>);
type SentDatagrams = Rc<RefCell<Vec<Datagram>>>;

struct MockSocket {
    incoming: VecDeque<Datagram>,
    sent: SentDatagrams,
}

fn mock_socket(incoming: impl IntoIterator<Item = Datagram>) -> (MockSocket, SentDatagrams) {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let socket = MockSocket {
        incoming: incoming.into_iter().collect(),
        sent: sent.clone(),
    };
    (socket, sent)
}

impl DatagramSocket for MockSocket {
    type Error = io::Error;

    async fn send_to(&mut self, destination: Endpoint, payload: &[u8]) -> io::Result<()> {
        self.sent.borrow_mut().push((destination, payload.to_vec()));
        Ok(())
    }

    async fn recv_from(&mut self, buffer: &mut [u8]) -> io::Result<DatagramMetadata> {
        let Some((_, payload)) = self.incoming.front() else {
            return std::future::pending().await;
        };
        if payload.len() > buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "buffer too small",
            ));
        }
        let (source, payload) = self.incoming.pop_front().unwrap();
        buffer[..payload.len()].copy_from_slice(&payload);
        Ok(DatagramMetadata::new(source, payload.len()))
    }
}

#[test]
fn message_encoding_matches_wire_bytes_and_round_trips() {
    assert_eq!(
        Message::Ping {
            request_id: 0x0102_0304
        }
        .encode(),
        [1, 1, 1, 2, 3, 4]
    );
    assert_eq!(
        Message::Executed {
            request_id: 0x0102_0304
        }
        .encode(),
        [1, 2, 1, 2, 3, 4]
    );
    for request_id in [0, 1, u32::MAX] {
        for message in [
            Message::Ping { request_id },
            Message::Executed { request_id },
        ] {
            assert_eq!(Message::decode(&message.encode()), Ok(message));
        }
    }
}

#[test]
fn malformed_messages_are_rejected_without_truncation() {
    for bytes in [
        vec![],
        vec![1; 5],
        vec![1; 7],
        vec![2, 1, 0, 0, 0, 1],
        vec![1, 3, 0, 0, 0, 1],
    ] {
        assert!(Message::decode(&bytes).is_err());
    }
}

#[tokio::test]
async fn sender_ignores_wrong_sources_stale_ids_and_non_confirmations() {
    let request_id = 42;
    let (mut socket, sent) = mock_socket([
        (
            TX_ENDPOINT,
            Message::Executed { request_id }.encode().to_vec(),
        ),
        (
            RX_ENDPOINT,
            Message::Executed { request_id: 41 }.encode().to_vec(),
        ),
        (RX_ENDPOINT, Message::Ping { request_id }.encode().to_vec()),
        (RX_ENDPOINT, vec![0; 6]),
        (
            RX_ENDPOINT,
            Message::Executed { request_id }.encode().to_vec(),
        ),
    ]);
    send_ping(&mut socket, RX_ENDPOINT, request_id)
        .await
        .unwrap();
    assert_eq!(
        sent.borrow().as_slice(),
        &[(RX_ENDPOINT, Message::Ping { request_id }.encode().to_vec())]
    );
}

#[tokio::test]
async fn missing_confirmation_times_out_without_retrying_command() {
    let (mut socket, sent) = mock_socket([]);
    let error = send_ping(&mut socket, RX_ENDPOINT, 42).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(sent.borrow().len(), 1);
}

#[test]
fn receiver_ignores_invalid_requests_and_waits_for_payload_execution_before_reply() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
    let request_id = 42;
    let (mut socket, sent) = mock_socket([
        (RX_ENDPOINT, Message::Ping { request_id }.encode().to_vec()),
        (
            TX_ENDPOINT,
            Message::Executed { request_id }.encode().to_vec(),
        ),
        (TX_ENDPOINT, vec![0; 7]),
        (TX_ENDPOINT, Message::Ping { request_id }.encode().to_vec()),
    ]);
    let mut future = std::pin::pin!(serve_commands(
        &mut socket,
        commands.dyn_sender(),
        events.dyn_receiver(),
    ));
    let mut context = Context::from_waker(Waker::noop());

    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(commands.try_receive(), Ok(PayloadCommand::Ping));
    assert!(commands.try_receive().is_err());
    assert!(sent.borrow().is_empty());

    events.try_send(PayloadEvent::Pong).unwrap();
    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(
        sent.borrow().as_slice(),
        &[(
            TX_ENDPOINT,
            Message::Executed { request_id }.encode().to_vec()
        )]
    );
    assert!(events.try_receive().is_err());
    assert!(commands.try_receive().is_err());
}

#[tokio::test]
async fn real_payload_executes_commands_and_confirms_them_over_csp_udp() {
    timeout(Duration::from_secs(5), async {
        let local = SocketAddr::from(([127, 0, 0, 1], 0));
        let mut tx_interface = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        let mut rx_interface = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        tx_interface.set_peers(&[rx_interface.local_addr().unwrap()]);
        rx_interface.set_peers(&[tx_interface.local_addr().unwrap()]);
        let tx_resources = StackResources::<MTU, 2, 1, 2>::new();
        let rx_resources = StackResources::<MTU, 2, 3, 2>::new();
        let mut tx_builder = RunnerBuilder::new(TX_NODE, tx_interface, &tx_resources).unwrap();
        let mut tx_socket = CspDatagramSocket::new(tx_builder.bind(COMMAND_PORT).unwrap());
        let mut tx_runner = tx_builder.build();
        let mut rx_builder = RunnerBuilder::new(RX_NODE, rx_interface, &rx_resources).unwrap();
        let beacon_socket = CspDatagramSocket::new(rx_builder.bind(BEACON_PORT).unwrap());
        let client_socket = CspDatagramSocket::new(rx_builder.bind(CLIENT_DATA_PORT).unwrap());
        let mut rx_socket = CspDatagramSocket::new(rx_builder.bind(COMMAND_PORT).unwrap());
        let mut rx_runner = rx_builder.build();
        let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
        let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
        let mut payload = PayloadSystem::new(
            beacon_socket,
            client_socket,
            RamStorage::<256, 16>::new(),
            events.dyn_sender(),
            commands.dyn_receiver(),
        );
        let application = async {
            for request_id in [1, 2, u32::MAX] {
                send_ping(&mut tx_socket, RX_ENDPOINT, request_id).await.unwrap();
            }
        };
        tokio::select! {
            () = application => {},
            result = serve_commands(&mut rx_socket, commands.dyn_sender(), events.dyn_receiver()) => panic!("bridge stopped: {result:?}"),
            result = tx_runner.run() => panic!("TX runner stopped: {result:?}"),
            result = rx_runner.run() => panic!("RX runner stopped: {result:?}"),
            _ = payload.run() => panic!("payload loop returned"),
        }
        assert!(commands.try_receive().is_err());
        assert!(events.try_receive().is_err());
    })
    .await
    .expect("CSP/UDP execution confirmation test timed out");
}

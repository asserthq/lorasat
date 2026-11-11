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
use sat_core::{
    net::{Address, DatagramMetadata, DatagramSocket, Endpoint},
    protocol::Beacon,
    storage::{QueueErrorKind, QueueMetadata, QueueStorage},
};
use sat_csp::{RunnerBuilder, StackResources};
use sat_drivers::storage::RamStorage;
use sat_subsystems::payload::{
    BeaconConfig, ClientRecordRejection, MAX_CLIENT_PAYLOAD_SIZE, MAX_CLIENT_RECORD_SIZE,
    PayloadCommand, PayloadEvent, PayloadSystem, client_record_codec,
};
use tokio::time::{Duration, timeout};

type Datagram = (Endpoint, Vec<u8>);
type SentDatagrams = Rc<RefCell<Vec<Datagram>>>;
type StoredRecords = Rc<RefCell<Vec<Vec<u8>>>>;

struct TracingStorage<'a, const DEPTH: usize> {
    inner: &'a mut RamStorage<MAX_CLIENT_RECORD_SIZE, DEPTH>,
    accepted: StoredRecords,
}

impl<const DEPTH: usize> QueueStorage for TracingStorage<'_, DEPTH> {
    type Error = <RamStorage<MAX_CLIENT_RECORD_SIZE, DEPTH> as QueueStorage>::Error;

    fn max_entry_size(&self) -> Option<usize> {
        self.inner.max_entry_size()
    }

    fn error_kind(error: &Self::Error) -> QueueErrorKind {
        RamStorage::<MAX_CLIENT_RECORD_SIZE, DEPTH>::error_kind(error)
    }

    async fn append(&mut self, entry: &[u8]) -> Result<(), Self::Error> {
        self.inner.append(entry).await?;
        self.accepted.borrow_mut().push(entry.to_vec());
        Ok(())
    }

    async fn peek(&mut self, buffer: &mut [u8]) -> Result<Option<QueueMetadata>, Self::Error> {
        self.inner.peek(buffer).await
    }

    async fn pop(&mut self) -> Result<bool, Self::Error> {
        self.inner.pop().await
    }

    async fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear().await
    }
}

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

    events
        .try_send(PayloadEvent::BeaconSent {
            destination: RX_ENDPOINT,
        })
        .unwrap();
    assert!(future.as_mut().poll(&mut context).is_pending());
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
    timeout(Duration::from_secs(15), async {
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
        let broadcast = Endpoint::broadcast(BEACON_PORT);
        let config = BeaconConfig::new(Address::new_unicast(RX_NODE).unwrap(), broadcast);
        let mut payload = PayloadSystem::new(
            beacon_socket,
            client_socket,
            RamStorage::<MAX_CLIENT_RECORD_SIZE, 16>::new(),
            events.dyn_sender(),
            commands.dyn_receiver(),
            config,
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

#[tokio::test]
async fn payload_asp_beacon_waits_ten_seconds_and_does_not_block_commands() {
    timeout(Duration::from_secs(15), async {
        let local = SocketAddr::from(([127, 0, 0, 1], 0));
        let mut tx_interface = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        let mut rx_interface = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        tx_interface.set_peers(&[rx_interface.local_addr().unwrap()]);
        rx_interface.set_peers(&[tx_interface.local_addr().unwrap()]);
        let tx_resources = StackResources::<MTU, 2, 2, 2>::new();
        let rx_resources = StackResources::<MTU, 2, 3, 2>::new();
        let mut tx_builder = RunnerBuilder::new(TX_NODE, tx_interface, &tx_resources).unwrap();
        let mut tx_socket = CspDatagramSocket::new(tx_builder.bind(COMMAND_PORT).unwrap());
        let mut received_beacons = CspDatagramSocket::new(tx_builder.bind(BEACON_PORT).unwrap());
        let mut tx_runner = tx_builder.build();
        let mut rx_builder = RunnerBuilder::new(RX_NODE, rx_interface, &rx_resources).unwrap();
        let beacon_socket = CspDatagramSocket::new(rx_builder.bind(BEACON_PORT).unwrap());
        let client_socket = CspDatagramSocket::new(rx_builder.bind(CLIENT_DATA_PORT).unwrap());
        let mut rx_socket = CspDatagramSocket::new(rx_builder.bind(COMMAND_PORT).unwrap());
        let mut rx_runner = rx_builder.build();
        let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
        let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
        let node_address = Address::new_unicast(RX_NODE).unwrap();
        let broadcast = Endpoint::broadcast(BEACON_PORT);
        let config = BeaconConfig::new(node_address, broadcast);
        assert_eq!(config.satellite_address, node_address);
        assert_eq!(config.destination, broadcast);
        assert_eq!(config.interval_seconds.get(), 10);
        let mut payload = PayloadSystem::new(
            beacon_socket,
            client_socket,
            RamStorage::<MAX_CLIENT_RECORD_SIZE, 16>::new(),
            events.dyn_sender(),
            commands.dyn_receiver(),
            config,
        );
        assert_eq!(payload.state().transmitted_beacons, 0);
        let started_at = embassy_time::Instant::now().as_millis();
        let application = async {
            let mut buffer = [0; MTU];
            let quiet = async {
                assert!(
                    timeout(Duration::from_secs(9), received_beacons.recv_from(&mut buffer))
                        .await
                        .is_err(),
                    "ASP beacon arrived before nine seconds"
                );
            };
            tokio::join!(quiet, async {
                send_ping(&mut tx_socket, RX_ENDPOINT, 1).await.unwrap();
            });
            let metadata = timeout(Duration::from_secs(4), received_beacons.recv_from(&mut buffer))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(metadata.source(), Endpoint::new(node_address, BEACON_PORT));
            let beacon = Beacon::decode(&buffer[..metadata.length()]).unwrap();
            assert_eq!(beacon.sat_addr, node_address);
            assert_eq!(beacon.interval_sec, config.interval_seconds.get());
            assert!(beacon.timestamp >= started_at + 10_000);
            assert!(beacon.timestamp <= embassy_time::Instant::now().as_millis());
            send_ping(&mut tx_socket, RX_ENDPOINT, 2).await.unwrap();
            assert!(
                timeout(Duration::from_millis(100), received_beacons.recv_from(&mut buffer))
                    .await
                    .is_err(),
                "unexpected second ASP beacon in the same interval"
            );
        };
        tokio::select! {
            () = application => {},
            result = serve_commands(&mut rx_socket, commands.dyn_sender(), events.dyn_receiver()) => panic!("bridge stopped: {result:?}"),
            result = tx_runner.run() => panic!("TX runner stopped: {result:?}"),
            result = rx_runner.run() => panic!("RX runner stopped: {result:?}"),
            _ = payload.run() => panic!("payload loop returned"),
        }
        assert_eq!(payload.state().transmitted_beacons, 1);
        assert_eq!(payload.state().beacon_send_errors, 0);
    }).await.expect("CSP/UDP payload beacon test timed out");
}

#[tokio::test]
async fn payload_stores_client_records_over_csp_udp_and_preserves_full_fifo() {
    timeout(Duration::from_secs(5), async {
        let local = SocketAddr::from(([127, 0, 0, 1], 0));
        let mut tx_interface = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        let mut rx_interface = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        tx_interface.set_peers(&[rx_interface.local_addr().unwrap()]);
        rx_interface.set_peers(&[tx_interface.local_addr().unwrap()]);
        let tx_resources = StackResources::<MTU, 2, 2, 2>::new();
        let rx_resources = StackResources::<MTU, 2, 2, 2>::new();
        let mut tx_builder = RunnerBuilder::new(TX_NODE, tx_interface, &tx_resources).unwrap();
        let mut data_sender = CspDatagramSocket::new(tx_builder.bind(CLIENT_DATA_PORT).unwrap());
        let mut other_sender = CspDatagramSocket::new(tx_builder.bind(COMMAND_PORT).unwrap());
        let mut tx_runner = tx_builder.build();
        let mut rx_builder = RunnerBuilder::new(RX_NODE, rx_interface, &rx_resources).unwrap();
        let client_beacon_socket = CspDatagramSocket::new(rx_builder.bind(BEACON_PORT).unwrap());
        let client_socket = CspDatagramSocket::new(rx_builder.bind(CLIENT_DATA_PORT).unwrap());
        let mut rx_runner = rx_builder.build();
        let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
        let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
        let broadcast = Endpoint::broadcast(BEACON_PORT);
        let config = BeaconConfig::new(Address::new_unicast(RX_NODE).unwrap(), broadcast);
        let mut ram = RamStorage::<MAX_CLIENT_RECORD_SIZE, 16>::new();
        let accepted = StoredRecords::default();
        let storage = TracingStorage {
            inner: &mut ram,
            accepted: accepted.clone(),
        };
        let mut payload = PayloadSystem::new(
            client_beacon_socket,
            client_socket,
            storage,
            events.dyn_sender(),
            commands.dyn_receiver(),
            config,
        );
        let initial = *payload.state();
        assert_eq!(initial.received_datagrams, 0);
        assert_eq!(initial.stored_records, 0);
        assert_eq!(initial.rejected_records, 0);
        assert_eq!(initial.receive_errors, 0);
        assert_eq!(initial.storage_errors, 0);
        assert_eq!(initial.last_client, None);
        assert_eq!(MAX_CLIENT_PAYLOAD_SIZE, 256);
        assert_eq!(client_record_codec::HEADER_SIZE, 16);
        assert_eq!(MAX_CLIENT_RECORD_SIZE, 272);
        let maximum_csp_payload = MTU - sat_csp::HEADER_SIZE;
        assert_eq!(maximum_csp_payload, 249);
        let mut contents = vec![
            vec![0, 255, 128, 0, 1],
            vec![],
            (0..maximum_csp_payload).map(|byte| byte as u8).collect(),
        ];
        for index in contents.len()..16 {
            contents.push(vec![index as u8, 0, 255]);
        }
        let destination = Endpoint::new(Address::new_unicast(RX_NODE).unwrap(), CLIENT_DATA_PORT);
        let sources = [
            Endpoint::new(Address::new_unicast(TX_NODE).unwrap(), CLIENT_DATA_PORT),
            TX_ENDPOINT,
        ];
        let application = async {
            commands.send(PayloadCommand::GetState).await;
            assert_eq!(events.receive().await, PayloadEvent::State(initial));
            commands.send(PayloadCommand::Ping).await;
            assert_eq!(events.receive().await, PayloadEvent::Pong);
            let mut received_windows = Vec::new();
            for (index, bytes) in contents.iter().enumerate() {
                let before = embassy_time::Instant::now().as_millis();
                let sender = if index % 2 == 0 {
                    &mut data_sender
                } else {
                    &mut other_sender
                };
                sender.send_to(destination, bytes).await.unwrap();
                assert_eq!(
                    events.receive().await,
                    PayloadEvent::ClientRecordStored {
                        source: sources[index % 2],
                        payload_length: bytes.len(),
                    }
                );
                received_windows.push((before, embassy_time::Instant::now().as_millis()));
                assert_eq!(accepted.borrow().len(), index + 1);
            }
            commands.send(PayloadCommand::GetState).await;
            let PayloadEvent::State(full) = events.receive().await else {
                panic!("expected state after filling FIFO");
            };
            assert_eq!(full.received_datagrams, 16);
            assert_eq!(full.stored_records, 16);
            assert_eq!(full.rejected_records, 0);
            assert_eq!(full.storage_errors, 0);
            assert_eq!(full.receive_errors, 0);
            assert_eq!(full.last_client, Some(sources[1]));
            for (index, bytes) in [vec![254, 0, 255], vec![]].iter().enumerate() {
                let sender = if index % 2 == 0 {
                    &mut data_sender
                } else {
                    &mut other_sender
                };
                sender.send_to(destination, bytes).await.unwrap();
                assert_eq!(
                    events.receive().await,
                    PayloadEvent::ClientRecordRejected {
                        source: sources[index % 2],
                        payload_length: bytes.len(),
                        reason: ClientRecordRejection::Storage(QueueErrorKind::Full),
                    }
                );
                assert_eq!(accepted.borrow().len(), 16);
                commands.send(PayloadCommand::GetState).await;
                let PayloadEvent::State(rejected) = events.receive().await else {
                    panic!("expected state after FIFO rejection");
                };
                assert_eq!(rejected.received_datagrams, 17 + index as u64);
                assert_eq!(rejected.stored_records, 16);
                assert_eq!(rejected.rejected_records, 1 + index as u64);
                assert_eq!(rejected.storage_errors, 1 + index as u64);
                assert_eq!(rejected.last_client, full.last_client);
            }
            commands.send(PayloadCommand::Ping).await;
            assert_eq!(events.receive().await, PayloadEvent::Pong);
            commands.send(PayloadCommand::GetState).await;
            let PayloadEvent::State(state) = events.receive().await else {
                panic!("expected state after rejecting records");
            };
            assert_eq!(state.received_datagrams, 18);
            assert_eq!(state.stored_records, 16);
            assert_eq!(state.rejected_records, 2);
            assert_eq!(state.storage_errors, 2);
            assert_eq!(state.receive_errors, 0);
            assert_eq!(state.dropped_notifications, 0);
            assert_eq!(state.last_client, Some(sources[1]));
            (state, received_windows)
        };
        let (state, received_windows) = tokio::select! {
            result = application => result,
            result = tx_runner.run() => panic!("TX runner stopped: {result:?}"),
            result = rx_runner.run() => panic!("RX runner stopped: {result:?}"),
            _ = payload.run() => panic!("payload loop returned"),
        };
        assert_eq!(*payload.state(), state);
        drop(payload);
        assert_eq!(ram.len(), 16);
        let records = accepted.borrow().clone();
        assert_eq!(records.len(), 16);
        let mut previous_timestamp = 0;
        let mut buffer = [0; MAX_CLIENT_RECORD_SIZE];
        for (index, encoded) in records.iter().enumerate() {
            let metadata = ram.peek(&mut buffer).await.unwrap().unwrap();
            assert_eq!(&buffer[..metadata.length()], encoded.as_slice());
            let record = client_record_codec::decode(encoded).unwrap();
            assert_eq!(record.source, sources[index % 2]);
            assert_eq!(record.payload, contents[index].as_slice());
            assert_eq!(
                encoded.len(),
                client_record_codec::HEADER_SIZE + contents[index].len()
            );
            let (before, after) = received_windows[index];
            assert!(record.received_at_ms >= before);
            assert!(record.received_at_ms <= after);
            assert!(record.received_at_ms >= previous_timestamp);
            previous_timestamp = record.received_at_ms;
            assert!(ram.pop().await.unwrap());
        }
        assert!(ram.peek(&mut buffer).await.unwrap().is_none());
        assert!(!ram.pop().await.unwrap());
        assert!(commands.try_receive().is_err());
        assert!(events.try_receive().is_err());
    })
    .await
    .expect("CSP/UDP client record test timed out");
}

#[tokio::test]
async fn payload_discards_oversized_csp_datagram_and_stores_next_without_losing_old_records() {
    timeout(Duration::from_secs(5), async {
        const TEST_MTU: usize = 320;

        let local = SocketAddr::from(([127, 0, 0, 1], 0));
        let mut tx_interface = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        let mut rx_interface = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        tx_interface.set_peers(&[rx_interface.local_addr().unwrap()]);
        rx_interface.set_peers(&[tx_interface.local_addr().unwrap()]);
        let tx_resources = StackResources::<TEST_MTU, 2, 2, 2>::new();
        let rx_resources = StackResources::<TEST_MTU, 2, 2, 2>::new();
        let mut tx_builder = RunnerBuilder::new(TX_NODE, tx_interface, &tx_resources).unwrap();
        let mut oversized_sender = CspDatagramSocket::new(tx_builder.bind(COMMAND_PORT).unwrap());
        let mut valid_sender = CspDatagramSocket::new(tx_builder.bind(CLIENT_DATA_PORT).unwrap());
        let mut tx_runner = tx_builder.build();
        let mut rx_builder = RunnerBuilder::new(RX_NODE, rx_interface, &rx_resources).unwrap();
        let client_beacon_socket = CspDatagramSocket::new(rx_builder.bind(BEACON_PORT).unwrap());
        let client_socket = CspDatagramSocket::new(rx_builder.bind(CLIENT_DATA_PORT).unwrap());
        let mut rx_runner = rx_builder.build();
        let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
        let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
        let broadcast = Endpoint::broadcast(BEACON_PORT);
        let config = BeaconConfig::new(Address::new_unicast(RX_NODE).unwrap(), broadcast);
        assert_eq!(config.interval_seconds.get(), 10);
        let mut ram = RamStorage::<MAX_CLIENT_RECORD_SIZE, 16>::new();
        let mut buffer = [0; MAX_CLIENT_RECORD_SIZE];
        let old_source = Endpoint::new(Address::new_unicast(3).unwrap(), CLIENT_DATA_PORT);
        let old_payload = [0, 255, 42];
        let old_length =
            client_record_codec::encode(old_source, 7, &old_payload, &mut buffer).unwrap();
        let old_record = buffer[..old_length].to_vec();
        ram.append(&old_record).await.unwrap();
        let accepted = StoredRecords::default();
        let storage = TracingStorage {
            inner: &mut ram,
            accepted: accepted.clone(),
        };
        let mut payload = PayloadSystem::new(
            client_beacon_socket,
            client_socket,
            storage,
            events.dyn_sender(),
            commands.dyn_receiver(),
            config,
        );
        let destination = Endpoint::new(Address::new_unicast(RX_NODE).unwrap(), CLIENT_DATA_PORT);
        let valid_source = Endpoint::new(Address::new_unicast(TX_NODE).unwrap(), CLIENT_DATA_PORT);
        let oversized = [255; MAX_CLIENT_PAYLOAD_SIZE + 1];
        assert_eq!(oversized.len(), 257);
        assert!(oversized.len() + sat_csp::HEADER_SIZE <= TEST_MTU);
        let mut valid = [128; MAX_CLIENT_PAYLOAD_SIZE];
        valid[..3].copy_from_slice(&[0, 255, 1]);
        let application = async {
            oversized_sender
                .send_to(destination, &oversized)
                .await
                .unwrap();
            assert_eq!(
                events.receive().await,
                PayloadEvent::ClientRecordRejected {
                    source: TX_ENDPOINT,
                    payload_length: 257,
                    reason: ClientRecordRejection::PayloadTooLong,
                }
            );
            assert!(accepted.borrow().is_empty());
            commands.send(PayloadCommand::GetState).await;
            let PayloadEvent::State(rejected) = events.receive().await else {
                panic!("expected state after oversized datagram rejection");
            };
            assert_eq!(rejected.received_datagrams, 1);
            assert_eq!(rejected.rejected_records, 1);
            assert_eq!(rejected.stored_records, 0);
            assert_eq!(rejected.receive_errors, 0);
            assert_eq!(rejected.storage_errors, 0);
            assert_eq!(rejected.last_client, None);
            let before = embassy_time::Instant::now().as_millis();
            valid_sender.send_to(destination, &valid).await.unwrap();
            assert_eq!(
                events.receive().await,
                PayloadEvent::ClientRecordStored {
                    source: valid_source,
                    payload_length: valid.len(),
                }
            );
            let after = embassy_time::Instant::now().as_millis();
            commands.send(PayloadCommand::GetState).await;
            let PayloadEvent::State(state) = events.receive().await else {
                panic!("expected state after valid datagram storage");
            };
            assert_eq!(state.received_datagrams, 2);
            assert_eq!(state.rejected_records, 1);
            assert_eq!(state.stored_records, 1);
            assert_eq!(state.receive_errors, 0);
            assert_eq!(state.storage_errors, 0);
            assert_eq!(state.dropped_notifications, 0);
            assert_eq!(state.last_client, Some(valid_source));
            (state, before, after)
        };
        let (state, before, after) = tokio::select! {
            result = application => result,
            result = tx_runner.run() => panic!("TX runner stopped: {result:?}"),
            result = rx_runner.run() => panic!("RX runner stopped: {result:?}"),
            _ = payload.run() => panic!("payload loop returned"),
        };
        assert_eq!(*payload.state(), state);
        drop(payload);
        assert_eq!(ram.len(), 2);
        let metadata = ram.peek(&mut buffer).await.unwrap().unwrap();
        assert_eq!(&buffer[..metadata.length()], old_record.as_slice());
        assert!(ram.pop().await.unwrap());
        let records = accepted.borrow().clone();
        assert_eq!(records.len(), 1);
        let metadata = ram.peek(&mut buffer).await.unwrap().unwrap();
        assert_eq!(&buffer[..metadata.length()], records[0].as_slice());
        let record = client_record_codec::decode(&buffer[..metadata.length()]).unwrap();
        assert_eq!(record.source, valid_source);
        assert_eq!(record.payload, valid);
        assert!(record.received_at_ms >= before);
        assert!(record.received_at_ms <= after);
        assert!(ram.pop().await.unwrap());
        assert!(ram.peek(&mut buffer).await.unwrap().is_none());
        assert!(commands.try_receive().is_err());
        assert!(events.try_receive().is_err());
    })
    .await
    .expect("CSP/UDP oversized datagram regression test timed out");
}

#[tokio::test]
async fn payload_pauses_client_reception_and_resumes_retained_csp_udp_fifo() {
    timeout(Duration::from_secs(5), async {
        let local = SocketAddr::from(([127, 0, 0, 1], 0));
        let mut tx_interface = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        let mut rx_interface = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        tx_interface.set_peers(&[rx_interface.local_addr().unwrap()]);
        rx_interface.set_peers(&[tx_interface.local_addr().unwrap()]);
        let tx_resources = StackResources::<MTU, 2, 1, 2>::new();
        let rx_resources = StackResources::<MTU, 2, 2, 2>::new();
        let mut tx_builder = RunnerBuilder::new(TX_NODE, tx_interface, &tx_resources).unwrap();
        let mut data_sender = CspDatagramSocket::new(tx_builder.bind(CLIENT_DATA_PORT).unwrap());
        let mut tx_runner = tx_builder.build();
        let mut rx_builder = RunnerBuilder::new(RX_NODE, rx_interface, &rx_resources).unwrap();
        let beacon_socket = CspDatagramSocket::new(rx_builder.bind(BEACON_PORT).unwrap());
        let client_socket = CspDatagramSocket::new(rx_builder.bind(CLIENT_DATA_PORT).unwrap());
        let mut rx_runner = rx_builder.build();
        let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
        let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
        let config = BeaconConfig::new(
            Address::new_unicast(RX_NODE).unwrap(),
            Endpoint::broadcast(BEACON_PORT),
        );
        assert_eq!(config.interval_seconds.get(), 10);
        let mut ram = RamStorage::<MAX_CLIENT_RECORD_SIZE, 2>::new();
        let accepted = StoredRecords::default();
        let storage = TracingStorage {
            inner: &mut ram,
            accepted: accepted.clone(),
        };
        let mut payload = PayloadSystem::new(
            beacon_socket,
            client_socket,
            storage,
            events.dyn_sender(),
            commands.dyn_receiver(),
            config,
        );
        let initial = *payload.state();
        assert!(initial.reception_enabled);
        assert_eq!(initial.received_datagrams, 0);
        assert_eq!(initial.stored_records, 0);
        let destination = Endpoint::new(Address::new_unicast(RX_NODE).unwrap(), CLIENT_DATA_PORT);
        let source = Endpoint::new(Address::new_unicast(TX_NODE).unwrap(), CLIENT_DATA_PORT);
        let contents = [vec![0, 255, 128, 0, 1], vec![254, 0, 42, 255]];
        let application = async {
            let pause = PayloadCommand::SetReceptionEnabled(false);
            commands.send(pause).await;
            assert_eq!(
                events.receive().await,
                PayloadEvent::CommandCompleted(pause)
            );
            let mut paused = initial;
            paused.reception_enabled = false;
            commands.send(PayloadCommand::GetState).await;
            assert_eq!(events.receive().await, PayloadEvent::State(paused));
            for bytes in &contents {
                data_sender.send_to(destination, bytes).await.unwrap();
            }
            let sent_at = embassy_time::Instant::now().as_millis();
            assert!(
                timeout(Duration::from_millis(100), events.receive())
                    .await
                    .is_err(),
                "unexpected payload event while reception was paused"
            );
            assert!(accepted.borrow().is_empty());
            commands.send(PayloadCommand::GetState).await;
            assert_eq!(events.receive().await, PayloadEvent::State(paused));
            commands.send(PayloadCommand::Ping).await;
            assert_eq!(events.receive().await, PayloadEvent::Pong);
            let resume = PayloadCommand::SetReceptionEnabled(true);
            let resumed_at = embassy_time::Instant::now().as_millis();
            assert!(resumed_at > sent_at);
            commands.send(resume).await;
            assert_eq!(
                events.receive().await,
                PayloadEvent::CommandCompleted(resume)
            );
            let mut stored_at = Vec::new();
            for (index, bytes) in contents.iter().enumerate() {
                assert_eq!(
                    events.receive().await,
                    PayloadEvent::ClientRecordStored {
                        source,
                        payload_length: bytes.len(),
                    }
                );
                stored_at.push(embassy_time::Instant::now().as_millis());
                let records = accepted.borrow();
                assert!(records.len() > index);
                let record = client_record_codec::decode(&records[index]).unwrap();
                assert_eq!(record.source, source);
                assert_eq!(record.payload, bytes.as_slice());
                assert!(record.received_at_ms >= resumed_at);
                assert!(record.received_at_ms <= stored_at[index]);
            }
            commands.send(PayloadCommand::GetState).await;
            let PayloadEvent::State(state) = events.receive().await else {
                panic!("expected state after resuming client reception");
            };
            let mut expected = initial;
            expected.received_datagrams = 2;
            expected.stored_records = 2;
            expected.last_client = Some(source);
            assert_eq!(state, expected);
            commands.send(PayloadCommand::Ping).await;
            assert_eq!(events.receive().await, PayloadEvent::Pong);
            (state, resumed_at, stored_at)
        };
        let (state, resumed_at, stored_at) = tokio::select! {
            result = application => result,
            result = tx_runner.run() => panic!("TX runner stopped: {result:?}"),
            result = rx_runner.run() => panic!("RX runner stopped: {result:?}"),
            _ = payload.run() => panic!("payload loop returned"),
        };
        assert_eq!(*payload.state(), state);
        drop(payload);
        assert_eq!(ram.len(), 2);
        let records = accepted.borrow().clone();
        assert_eq!(records.len(), 2);
        let mut previous_timestamp = resumed_at;
        let mut buffer = [0; MAX_CLIENT_RECORD_SIZE];
        for (index, encoded) in records.iter().enumerate() {
            let metadata = ram.peek(&mut buffer).await.unwrap().unwrap();
            assert_eq!(&buffer[..metadata.length()], encoded.as_slice());
            let record = client_record_codec::decode(&buffer[..metadata.length()]).unwrap();
            assert_eq!(record.source, source);
            assert_eq!(record.payload, contents[index].as_slice());
            assert_eq!(
                encoded.len(),
                client_record_codec::HEADER_SIZE + contents[index].len()
            );
            assert!(record.received_at_ms >= previous_timestamp);
            assert!(record.received_at_ms <= stored_at[index]);
            previous_timestamp = record.received_at_ms;
            assert!(ram.pop().await.unwrap());
        }
        assert!(ram.peek(&mut buffer).await.unwrap().is_none());
        assert!(!ram.pop().await.unwrap());
        assert!(commands.try_receive().is_err());
        assert!(events.try_receive().is_err());
    })
    .await
    .expect("CSP/UDP reception pause/resume test timed out");
}

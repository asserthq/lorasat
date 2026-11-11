extern crate std;

use super::*;
use core::{
    future::{Future, poll_fn},
    num::NonZeroU16,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel};
use sat_core::{
    domain::record::ClientRecord,
    net::{Address, DatagramMetadata, Endpoint},
    protocol::Beacon,
    storage::{QueueErrorKind, QueueMetadata},
};
use sat_drivers::storage::RamStorage;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    vec::Vec,
};

use super::super::{
    ClientRecordRejection, DEFAULT_BEACON_INTERVAL_SECONDS, MAX_CLIENT_PAYLOAD_SIZE,
    MAX_CLIENT_RECORD_SIZE, client_record_codec,
};

fn run_ready<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("operation must complete immediately"),
    }
}

fn drive(mut future: Pin<&mut impl Future>) {
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..32 {
        assert!(future.as_mut().poll(&mut context).is_pending());
    }
}

struct MockSocket<Marker>(Marker);

impl<Marker> DatagramSocket for MockSocket<Marker> {
    type Error = ();

    async fn send_to(
        &mut self,
        _destination: Endpoint,
        _payload: &[u8],
    ) -> Result<(), Self::Error> {
        panic!("unexpected datagram send");
    }

    async fn recv_from(&mut self, _buffer: &mut [u8]) -> Result<DatagramMetadata, Self::Error> {
        core::future::pending().await
    }
}

type MockPayload<'a> =
    PayloadSystem<'a, MockSocket<&'static str>, MockSocket<u8>, RamStorage<32, 2>>;

fn mock_payload<'a>(
    events: DynamicSender<'a, PayloadEvent>,
    commands: DynamicReceiver<'a, PayloadCommand>,
    storage: RamStorage<32, 2>,
) -> MockPayload<'a> {
    PayloadSystem::new(
        MockSocket("ASP beacon"),
        MockSocket(7_u8),
        storage,
        events,
        commands,
        config(),
    )
}

#[test]
fn run_sends_one_pong_per_ping_and_continues_waiting() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let mut storage = RamStorage::<32, 2>::new();
    run_ready(storage.append(b"retained")).unwrap();
    let mut payload = mock_payload(events.dyn_sender(), commands.dyn_receiver(), storage);
    commands.try_send(PayloadCommand::Ping).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        for _ in 0..2 {
            assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        }
        assert!(events.try_receive().is_err());
        assert!(commands.try_receive().is_err());
        commands.try_send(PayloadCommand::Ping).unwrap();
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        assert!(events.try_receive().is_err());
    }
    assert_eq!(payload.storage.len(), 1);
    let mut buffer = [0; 32];
    let metadata = run_ready(payload.storage.peek(&mut buffer))
        .unwrap()
        .unwrap();
    assert_eq!(&buffer[..metadata.length()], b"retained");
}

#[test]
fn full_event_channel_defers_next_command_without_dropping_events() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
    let mut payload = mock_payload(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::new(),
    );
    events.try_send(PayloadEvent::Pong).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
    drive(future.as_mut());
    assert_eq!(commands.len(), 1);
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    drive(future.as_mut());
    assert_eq!(commands.len(), 0);
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    drive(future.as_mut());
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    assert!(events.try_receive().is_err());
}

#[test]
fn cancelled_run_retains_pending_event_for_restart_without_duplicates() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
    let mut payload = mock_payload(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::new(),
    );
    events.try_send(PayloadEvent::Pong).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
    }
    assert_eq!(payload.pending_event, Some(PayloadEvent::Pong));
    assert!(commands.try_receive().is_err());
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
    }
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    assert!(events.try_receive().is_err());
    assert_eq!(payload.pending_event, None);
}

#[test]
fn ping_returns_pong_and_consumes_only_one_command() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let mut payload = mock_payload(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::new(),
    );
    commands.try_send(PayloadCommand::Ping).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();
    assert_eq!(
        run_ready(payload.process_next_command()),
        PayloadEvent::Pong
    );
    assert_eq!(commands.try_receive(), Ok(PayloadCommand::Ping));
    assert!(commands.try_receive().is_err());
    assert!(events.try_receive().is_err());
    assert!(payload.storage.is_empty());
}

#[test]
fn processing_commands_preserves_storage_without_sending_events() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let mut storage = RamStorage::<32, 2>::new();
    run_ready(storage.append(b"retained")).unwrap();
    let mut payload = mock_payload(events.dyn_sender(), commands.dyn_receiver(), storage);
    for _ in 0..4 {
        commands.try_send(PayloadCommand::Ping).unwrap();
        assert_eq!(
            run_ready(payload.process_next_command()),
            PayloadEvent::Pong
        );
        assert!(commands.try_receive().is_err());
        assert!(events.try_receive().is_err());
        assert_eq!(payload.storage.len(), 1);
        let mut buffer = [0; 32];
        let metadata = run_ready(payload.storage.peek(&mut buffer))
            .unwrap()
            .unwrap();
        assert_eq!(&buffer[..metadata.length()], b"retained");
    }
}

#[test]
fn empty_command_channel_waits_and_cancellation_keeps_receiver_usable() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let mut payload = mock_payload(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::new(),
    );
    {
        let mut future = core::pin::pin!(payload.process_next_command());
        let mut context = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut context).is_pending());
    }
    assert!(events.try_receive().is_err());
    commands.try_send(PayloadCommand::Ping).unwrap();
    assert_eq!(
        run_ready(payload.process_next_command()),
        PayloadEvent::Pong
    );
    assert!(commands.try_receive().is_err());
    assert!(events.try_receive().is_err());
}

#[test]
fn constructor_stores_separate_protocol_handles_and_mandatory_config() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let payload = mock_payload(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::new(),
    );
    assert_eq!(payload.beacon_socket.0, "ASP beacon");
    assert_eq!(payload.client_socket.0, 7);
    assert!(payload.state().reception_enabled);
    assert_eq!(*payload.state(), PayloadState::new(config()));
    assert_eq!(payload.pending_event, None);
    assert_eq!(payload.next_beacon_at, None);
    assert!(commands.try_receive().is_err());
    assert!(events.try_receive().is_err());
}

#[test]
fn constructor_preserves_pending_messages() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    commands.try_send(PayloadCommand::Ping).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();
    events.try_send(PayloadEvent::Pong).unwrap();
    let _payload = mock_payload(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::new(),
    );
    assert_eq!(commands.try_receive(), Ok(PayloadCommand::Ping));
    assert_eq!(commands.try_receive(), Ok(PayloadCommand::Ping));
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    assert!(commands.try_receive().is_err());
    assert!(events.try_receive().is_err());
}

#[test]
fn stored_handles_use_the_provided_channels() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let payload = mock_payload(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::new(),
    );
    commands.try_send(PayloadCommand::Ping).unwrap();
    assert_eq!(payload.commands.try_receive(), Ok(PayloadCommand::Ping));
    assert!(payload.commands.try_receive().is_err());
    payload.events.try_send(PayloadEvent::Pong).unwrap();
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    assert!(events.try_receive().is_err());
}

#[test]
fn constructor_preserves_existing_queue_and_allows_appending() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let mut storage = RamStorage::<32, 2>::new();
    run_ready(storage.append(b"retained")).unwrap();
    let mut payload = mock_payload(events.dyn_sender(), commands.dyn_receiver(), storage);
    run_ready(payload.storage.append(b"new")).unwrap();
    assert_eq!(payload.storage.len(), 2);
    let mut buffer = [0; 32];
    for expected in [b"retained".as_slice(), b"new".as_slice()] {
        let metadata = run_ready(payload.storage.peek(&mut buffer))
            .unwrap()
            .unwrap();
        assert_eq!(&buffer[..metadata.length()], expected);
        assert!(run_ready(payload.storage.pop()).unwrap());
    }
    assert!(
        run_ready(payload.storage.peek(&mut buffer))
            .unwrap()
            .is_none()
    );
    assert!(commands.try_receive().is_err());
    assert!(events.try_receive().is_err());
}

#[derive(Clone)]
struct FakeTimer(Rc<Cell<u64>>);

impl Clock for FakeTimer {
    fn now_micros(&self) -> u64 {
        self.0.get()
    }
}

impl Delay for FakeTimer {
    async fn delay(&mut self, duration: Duration) {
        let deadline = self.0.get().saturating_add(duration.as_micros() as u64);
        poll_fn(|_| {
            if self.0.get() >= deadline {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

#[derive(Clone)]
struct FirstPollPendingTimer(Rc<Cell<u64>>);

impl Clock for FirstPollPendingTimer {
    fn now_micros(&self) -> u64 {
        self.0.get()
    }
}

impl Delay for FirstPollPendingTimer {
    async fn delay(&mut self, duration: Duration) {
        let deadline = self.0.get().saturating_add(duration.as_micros() as u64);
        let mut first_poll = true;
        poll_fn(|context| {
            if first_poll {
                first_poll = false;
                context.waker().wake_by_ref();
                Poll::Pending
            } else if self.0.get() >= deadline {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

#[derive(Default)]
struct SocketTrace {
    attempts: RefCell<Vec<(Endpoint, Vec<u8>)>>,
    blocked: Cell<bool>,
    fail: Cell<bool>,
    completed: Cell<usize>,
}

struct BeaconSocket(Rc<SocketTrace>);

impl DatagramSocket for BeaconSocket {
    type Error = ();

    fn send_to(
        &mut self,
        destination: Endpoint,
        payload: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>> {
        self.0
            .attempts
            .borrow_mut()
            .push((destination, payload.to_vec()));
        let trace = self.0.clone();
        async move {
            poll_fn(|_| {
                if trace.blocked.get() {
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;
            trace.completed.set(trace.completed.get() + 1);
            if trace.fail.get() { Err(()) } else { Ok(()) }
        }
    }

    async fn recv_from(&mut self, _buffer: &mut [u8]) -> Result<DatagramMetadata, Self::Error> {
        panic!("unexpected beacon receive");
    }
}

struct ClientSocket;

impl DatagramSocket for ClientSocket {
    type Error = ();

    async fn send_to(
        &mut self,
        _destination: Endpoint,
        _payload: &[u8],
    ) -> Result<(), Self::Error> {
        panic!("unexpected client send");
    }

    async fn recv_from(&mut self, _buffer: &mut [u8]) -> Result<DatagramMetadata, Self::Error> {
        core::future::pending().await
    }
}

type TestPayload<'a> = PayloadSystem<'a, BeaconSocket, ClientSocket, RamStorage<32, 2>>;

fn setup<'a>(
    events: DynamicSender<'a, PayloadEvent>,
    commands: DynamicReceiver<'a, PayloadCommand>,
) -> (TestPayload<'a>, Rc<SocketTrace>) {
    let beacon_trace = Rc::new(SocketTrace::default());
    let payload = PayloadSystem::new(
        BeaconSocket(beacon_trace.clone()),
        ClientSocket,
        RamStorage::new(),
        events,
        commands,
        config(),
    );
    (payload, beacon_trace)
}

fn config() -> BeaconConfig {
    BeaconConfig::new(
        Address::new(0x1234).unwrap(),
        Endpoint::new(Address::new(42).unwrap(), 7),
    )
}

fn replacement_config() -> BeaconConfig {
    BeaconConfig {
        satellite_address: Address::new(0x2345).unwrap(),
        destination: Endpoint::new(Address::new(99).unwrap(), 8),
        interval_seconds: NonZeroU16::new(3).unwrap(),
    }
}

fn notification(config: BeaconConfig, fail: bool) -> PayloadEvent {
    let destination = config.destination;
    if fail {
        PayloadEvent::BeaconSendFailed { destination }
    } else {
        PayloadEvent::BeaconSent { destination }
    }
}

fn assert_notifications(
    events: DynamicReceiver<'_, PayloadEvent>,
    config: BeaconConfig,
    failed: bool,
) {
    assert_eq!(events.try_receive(), Ok(notification(config, failed)));
    assert!(events.try_receive().is_err());
}

fn assert_beacon(trace: &SocketTrace, index: usize, config: BeaconConfig, timestamp_ms: u64) {
    let attempts = trace.attempts.borrow();
    let (actual_destination, bytes) = &attempts[index];
    let expected = Beacon {
        sat_addr: config.satellite_address,
        interval_sec: config.interval_seconds.get(),
        timestamp: timestamp_ms,
    };
    let mut buffer = [0; BEACON_BUFFER_SIZE];
    assert_eq!(*actual_destination, config.destination);
    assert_eq!(bytes.as_slice(), expected.encode(&mut buffer).unwrap());
    assert_eq!(Beacon::decode(bytes), Ok(expected));
}

fn assert_tick(trace: &SocketTrace, index: usize, config: BeaconConfig, timestamp_ms: u64) {
    assert_beacon(trace, index, config, timestamp_ms);
    assert_eq!(trace.attempts.borrow().len(), index + 1);
    assert_eq!(trace.completed.get(), index + 1);
}

fn assert_attempt_count(trace: &SocketTrace, count: usize) {
    assert_eq!(trace.attempts.borrow().len(), count);
}

#[test]
fn beacon_config_defaults_to_ten_nonzero_seconds() {
    let config = config();
    assert_eq!(config.satellite_address, Address::new(0x1234).unwrap());
    assert_eq!(
        config.destination,
        Endpoint::new(Address::new(42).unwrap(), 7)
    );
    assert_eq!(DEFAULT_BEACON_INTERVAL_SECONDS, 10);
    assert_eq!(config.interval_seconds, NonZeroU16::new(10).unwrap());
    assert!(NonZeroU16::new(0).is_none());
    assert_eq!(interval_micros(config), 10_000_000);
    let config = BeaconConfig {
        interval_seconds: NonZeroU16::new(u16::MAX).unwrap(),
        ..config
    };
    assert_eq!(interval_micros(config), 65_535_000_000);
}

#[test]
fn beacon_matches_native_postcard_wire_format() {
    let beacon = Beacon {
        sat_addr: Address::new(300).unwrap(),
        interval_sec: 10,
        timestamp: 700,
    };
    let bytes = [0xac, 0x02, 10, 0xbc, 0x05];
    let mut buffer = [0; BEACON_BUFFER_SIZE];
    assert_eq!(beacon.encode(&mut buffer).unwrap(), bytes);
    assert_eq!(Beacon::decode(&bytes), Ok(beacon));
}

#[test]
fn native_postcard_beacon_roundtrips_boundary_values() {
    for satellite_address in [
        Address::new(0).unwrap(),
        Address::new_unicast(Address::MAX_VALUE - 1).unwrap(),
        Address::BROADCAST,
    ] {
        for timestamp in [0, u64::MAX] {
            for interval_sec in [0, 1, 10, u16::MAX] {
                let beacon = Beacon {
                    sat_addr: satellite_address,
                    interval_sec,
                    timestamp,
                };
                let mut buffer = [0; BEACON_BUFFER_SIZE];
                let bytes = beacon.encode(&mut buffer).unwrap();
                assert_eq!(Beacon::decode(bytes), Ok(beacon));
            }
        }
    }
}

#[test]
fn maximal_valid_native_postcard_beacon_fits_conservative_stack_buffer() {
    let beacon = Beacon {
        sat_addr: Address::new(Address::MAX_VALUE).unwrap(),
        interval_sec: u16::MAX,
        timestamp: u64::MAX,
    };
    assert_eq!(BEACON_BUFFER_SIZE, 18);
    let mut buffer = [0; BEACON_BUFFER_SIZE];
    let bytes = beacon.encode(&mut buffer).unwrap();
    assert_eq!(bytes.len(), 15);
    assert_eq!(Beacon::decode(bytes), Ok(beacon));
    let mut undersized = [0; 14];
    assert!(beacon.encode(&mut undersized).is_err());
}

#[test]
fn payload_state_const_constructor_has_mandatory_config_and_zero_counters() {
    const CONFIG: BeaconConfig = BeaconConfig::new(
        Address::new(Address::MAX_VALUE).unwrap(),
        Endpoint::new(Address::new(42).unwrap(), 7),
    );
    const STATE: PayloadState = PayloadState::new(CONFIG);
    assert_eq!(STATE.beacon_config, CONFIG);
    assert!(STATE.reception_enabled);
    assert_eq!(STATE.transmitted_beacons, 0);
    assert_eq!(STATE.beacon_send_errors, 0);
    assert_eq!(STATE.dropped_notifications, 0);
    assert_eq!(STATE.received_datagrams, 0);
    assert_eq!(STATE.stored_records, 0);
    assert_eq!(STATE.rejected_records, 0);
    assert_eq!(STATE.receive_errors, 0);
    assert_eq!(STATE.storage_errors, 0);
    assert_eq!(STATE.last_client, None);
}

#[test]
fn new_payload_automatically_schedules_asp_beacon_after_a_full_interval() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    assert_eq!(*payload.state(), PayloadState::new(config()));
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        for micros in [0, 9_999_999] {
            now.set(micros);
            drive(future.as_mut());
            assert_attempt_count(&beacon_trace, 0);
            assert!(events.try_receive().is_err());
        }
        commands.try_send(PayloadCommand::GetState).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::State(PayloadState::new(config())))
        );
        now.set(10_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_notifications(events.dyn_receiver(), config(), false);
    }
    assert_eq!(payload.next_beacon_at, Some(20_000_000));
    assert_eq!(payload.state().transmitted_beacons, 1);
}

#[test]
fn configured_payload_sends_at_ten_twenty_and_thirty_seconds() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(5_000_000);
        commands.try_send(PayloadCommand::Ping).unwrap();
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        for (index, seconds) in [10_u64, 20, 30].into_iter().enumerate() {
            now.set(seconds * 1_000_000 - 1);
            drive(future.as_mut());
            assert_attempt_count(&beacon_trace, index);
            assert!(events.try_receive().is_err());
            now.set(seconds * 1_000_000);
            drive(future.as_mut());
            assert_tick(&beacon_trace, index, config(), seconds * 1_000);
            assert_notifications(events.dyn_receiver(), config(), false);
            drive(future.as_mut());
            assert_attempt_count(&beacon_trace, index + 1);
            assert!(events.try_receive().is_err());
        }
    }
    assert_eq!(payload.state().transmitted_beacons, 3);
    assert_eq!(payload.state().beacon_send_errors, 0);
    assert_eq!(payload.state().dropped_notifications, 0);
}

#[test]
fn beacon_timestamp_uses_clock_milliseconds_not_interval_count() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let now = Rc::new(Cell::new(1_234_567));
    let mut timer = FakeTimer(now.clone());
    let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
    drive(future.as_mut());
    now.set(11_234_566);
    drive(future.as_mut());
    assert_attempt_count(&beacon_trace, 0);
    now.set(11_234_567);
    drive(future.as_mut());
    assert_tick(&beacon_trace, 0, config(), 11_234);
    assert_notifications(events.dyn_receiver(), config(), false);
}

#[test]
fn configure_restarts_beacon_after_a_full_interval_from_command_time() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(5_000_000);
        let command = PayloadCommand::ConfigureBeacon(config());
        commands.try_send(command).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(command))
        );
        commands.try_send(PayloadCommand::GetState).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::State(PayloadState::new(config())))
        );
        for micros in [10_000_000, 14_999_999] {
            now.set(micros);
            drive(future.as_mut());
            assert_attempt_count(&beacon_trace, 0);
        }
        now.set(15_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 15_000);
        assert_notifications(events.dyn_receiver(), config(), false);
    }
    assert_eq!(payload.state().beacon_config, config());
    assert_eq!(payload.state().transmitted_beacons, 1);
    assert_eq!(payload.next_beacon_at, Some(25_000_000));
}

#[test]
fn configure_accepts_maximal_valid_address_and_maximal_nonzero_interval() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let config = BeaconConfig {
        satellite_address: Address::new(Address::MAX_VALUE).unwrap(),
        interval_seconds: NonZeroU16::new(u16::MAX).unwrap(),
        ..config()
    };
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        let command = PayloadCommand::ConfigureBeacon(config);
        commands.try_send(command).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(command))
        );
        now.set(65_534_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 0);
        now.set(65_535_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config, 65_535_000);
        assert_notifications(events.dyn_receiver(), config, false);
    }
    assert_eq!(payload.state().beacon_config, config);
    assert_eq!(payload.state().transmitted_beacons, 1);
    assert_eq!(payload.pending_event, None);
}

#[test]
fn reconfiguration_restarts_deadline_and_changes_beacon_contents() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let replacement = replacement_config();
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(5_000_000);
        let command = PayloadCommand::ConfigureBeacon(replacement);
        commands.try_send(command).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(command))
        );
        now.set(7_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 0);
        now.set(8_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, replacement, 8_000);
        assert_notifications(events.dyn_receiver(), replacement, false);
        now.set(10_000_000);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        now.set(11_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 1, replacement, 11_000);
        assert_notifications(events.dyn_receiver(), replacement, false);
    }
    assert_eq!(payload.state().beacon_config, replacement);
    assert_eq!(payload.state().transmitted_beacons, 2);
    assert_eq!(payload.next_beacon_at, Some(14_000_000));
}

#[test]
fn reconfiguring_identical_settings_restarts_a_full_interval_each_time() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        for seconds in [5, 10, 14] {
            now.set(seconds * 1_000_000);
            let command = PayloadCommand::ConfigureBeacon(config());
            commands.try_send(command).unwrap();
            drive(future.as_mut());
            assert_eq!(
                events.try_receive(),
                Ok(PayloadEvent::CommandCompleted(command))
            );
            assert_attempt_count(&beacon_trace, 0);
        }
        for micros in [15_000_000, 20_000_000, 23_999_999] {
            now.set(micros);
            drive(future.as_mut());
            assert_attempt_count(&beacon_trace, 0);
            assert!(events.try_receive().is_err());
        }
        now.set(24_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 24_000);
        assert_notifications(events.dyn_receiver(), config(), false);
    }
    assert_eq!(payload.state().transmitted_beacons, 1);
    assert_eq!(payload.next_beacon_at, Some(34_000_000));
}

#[test]
fn full_events_defer_commands_but_beacon_continues_and_reliable_replies_survive() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let filler = PayloadEvent::State(PayloadState::new(config()));
    events.try_send(filler).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();
    commands.try_send(PayloadCommand::GetState).unwrap();
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    let snapshot = PayloadState {
        reception_enabled: true,
        beacon_config: config(),
        transmitted_beacons: 3,
        beacon_send_errors: 0,
        dropped_notifications: 3,
        ..PayloadState::new(config())
    };
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_eq!(commands.len(), 1);
        for (index, seconds) in [10_u64, 20, 30].into_iter().enumerate() {
            now.set(seconds * 1_000_000);
            drive(future.as_mut());
            assert_tick(&beacon_trace, index, config(), seconds * 1_000);
            assert_eq!(commands.len(), 1);
            assert_eq!(events.len(), 1);
        }
        assert_eq!(events.try_receive(), Ok(filler));
        drive(future.as_mut());
        assert_eq!(commands.len(), 0);
    }
    assert_eq!(*payload.state(), snapshot);
    assert_eq!(payload.pending_event, Some(PayloadEvent::State(snapshot)));
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::State(snapshot)));
        drive(future.as_mut());
        assert!(events.try_receive().is_err());
    }
    assert_eq!(payload.pending_event, None);
    assert_attempt_count(&beacon_trace, 3);
}

#[test]
fn cancelled_run_retains_applied_command_reply_without_reapplying_command() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let command = PayloadCommand::ConfigureBeacon(config());
    events.try_send(PayloadEvent::Pong).unwrap();
    events.try_send(PayloadEvent::Pong).unwrap();
    commands.try_send(command).unwrap();
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
    }
    assert_eq!(
        payload.pending_event,
        Some(PayloadEvent::CommandCompleted(command))
    );
    assert_eq!(payload.next_beacon_at, Some(10_000_000));
    assert_eq!(commands.len(), 0);
    for _ in 0..2 {
        assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    }
    now.set(5_000_000);
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(command))
        );
        drive(future.as_mut());
        assert!(events.try_receive().is_err());
        now.set(10_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_notifications(events.dyn_receiver(), config(), false);
    }
    assert_eq!(payload.pending_event, None);
    assert_eq!(payload.state().transmitted_beacons, 1);
}

#[test]
fn pending_reliable_reply_has_priority_over_success_and_failure_notifications() {
    for failed in [false, true] {
        let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
        let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
        let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
        beacon_trace.fail.set(failed);
        let filler = PayloadEvent::State(PayloadState::new(config()));
        events.try_send(filler).unwrap();
        commands.try_send(PayloadCommand::Ping).unwrap();
        let now = Rc::new(Cell::new(0));
        let mut timer = FakeTimer(now.clone());
        {
            let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
            drive(future.as_mut());
            assert_eq!(commands.len(), 0);
            assert_eq!(events.try_receive(), Ok(filler));
            now.set(10_000_000);
            drive(future.as_mut());
            assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
            drive(future.as_mut());
            assert!(events.try_receive().is_err());
        }
        assert_attempt_count(&beacon_trace, 1);
        assert_eq!(payload.state().dropped_notifications, 1);
        assert_eq!(payload.state().transmitted_beacons, u64::from(!failed));
        assert_eq!(payload.state().beacon_send_errors, u64::from(failed));
        assert_eq!(payload.pending_event, None);
    }
}

#[test]
fn full_events_drop_only_best_effort_notifications_and_do_not_replay_them() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    for _ in 0..2 {
        events.try_send(PayloadEvent::Pong).unwrap();
    }
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(10_000_000);
        drive(future.as_mut());
        beacon_trace.fail.set(true);
        now.set(20_000_000);
        drive(future.as_mut());
        assert_eq!(beacon_trace.completed.get(), 2);
        beacon_trace.fail.set(false);
        for _ in 0..2 {
            assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        }
        drive(future.as_mut());
        assert!(events.try_receive().is_err());
        now.set(30_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 2, config(), 30_000);
        assert_notifications(events.dyn_receiver(), config(), false);
    }
    assert_eq!(payload.state().transmitted_beacons, 2);
    assert_eq!(payload.state().beacon_send_errors, 1);
    assert_eq!(payload.state().dropped_notifications, 2);
    assert_eq!(payload.pending_event, None);
}

#[test]
fn beacon_send_errors_leave_receiver_and_control_running_and_state_snapshot_is_immutable() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let (mut payload, beacon_trace, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    beacon_trace.fail.set(true);
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    let snapshot;
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(10_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_notifications(events.dyn_receiver(), config(), true);
        commands.try_send(PayloadCommand::GetState).unwrap();
        drive(future.as_mut());
        snapshot = state_event(events.dyn_receiver());
        assert_eq!(
            snapshot,
            PayloadState {
                reception_enabled: true,
                beacon_send_errors: 1,
                ..PayloadState::new(config())
            }
        );
        receive.packet(client_source(), b"independent");
        commands.try_send(PayloadCommand::Ping).unwrap();
        now.set(20_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 1, config(), 20_000);
        let actual: Vec<_> = (0..3).map(|_| events.try_receive().unwrap()).collect();
        assert!(actual.contains(&PayloadEvent::Pong));
        assert!(actual.contains(&notification(config(), true)));
        assert!(actual.contains(&stored(client_source(), 11)));
        assert!(events.try_receive().is_err());
        beacon_trace.fail.set(false);
        now.set(30_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 2, config(), 30_000);
        assert_notifications(events.dyn_receiver(), config(), false);
    }
    assert_eq!(
        snapshot,
        PayloadState {
            reception_enabled: true,
            beacon_send_errors: 1,
            ..PayloadState::new(config())
        }
    );
    assert_eq!(
        *payload.state(),
        PayloadState {
            reception_enabled: true,
            transmitted_beacons: 1,
            beacon_send_errors: 2,
            received_datagrams: 1,
            stored_records: 1,
            last_client: Some(client_source()),
            ..PayloadState::new(config())
        }
    );
    assert_client_record(
        &mut payload.storage,
        client_source(),
        20_000,
        b"independent",
    );
}

#[test]
fn ready_control_does_not_starve_beacon_when_delay_first_poll_is_pending() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 64>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 64>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let now = Rc::new(Cell::new(0));
    let mut timer = FirstPollPendingTimer(now.clone());
    let mut context = Context::from_waker(Waker::noop());
    let mut command_count = 0;
    let mut pong_count = 0;
    let mut snapshot_count = 0;
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        assert!(future.as_mut().poll(&mut context).is_pending());
        for micros in [
            9_000_000, 9_500_000, 9_999_999, 10_000_000, 10_000_001, 10_000_002, 10_000_003,
        ] {
            now.set(micros);
            while commands.len() < 64 {
                let command = if command_count % 2 == 0 {
                    PayloadCommand::Ping
                } else {
                    PayloadCommand::GetState
                };
                commands.try_send(command).unwrap();
                command_count += 1;
            }
            assert!(future.as_mut().poll(&mut context).is_pending());
            assert!(commands.len() > 0);
            while let Ok(event) = events.try_receive() {
                match event {
                    PayloadEvent::Pong => pong_count += 1,
                    PayloadEvent::State(_) => snapshot_count += 1,
                    PayloadEvent::BeaconSent { destination } => {
                        assert_eq!(destination, config().destination);
                    }
                    other => panic!("unexpected event: {other:?}"),
                }
            }
            if micros < 10_000_000 {
                assert_attempt_count(&beacon_trace, 0);
            }
        }
        assert!(pong_count > 0);
        assert!(snapshot_count > 0);
        assert_tick(&beacon_trace, 0, config(), 10_000);
    }
    assert_eq!(payload.state().transmitted_beacons, 1);
    assert_eq!(payload.state().beacon_send_errors, 0);
}

#[test]
fn repeated_configuration_during_stalled_sends_schedules_from_latest_command_time() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    beacon_trace.blocked.set(true);
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(10_000_000);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        for seconds in [11, 12] {
            now.set(seconds * 1_000_000);
            let command = PayloadCommand::ConfigureBeacon(config());
            commands.try_send(command).unwrap();
            drive(future.as_mut());
            assert_eq!(
                events.try_receive(),
                Ok(PayloadEvent::CommandCompleted(command))
            );
            assert_eq!(commands.len(), 0);
            assert_attempt_count(&beacon_trace, 1);
            assert_eq!(beacon_trace.completed.get(), 0);
        }
        now.set(15_000_000);
        beacon_trace.blocked.set(false);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_notifications(events.dyn_receiver(), config(), false);
        now.set(21_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        assert!(events.try_receive().is_err());
        now.set(22_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 1, config(), 22_000);
        assert_notifications(events.dyn_receiver(), config(), false);
    }
    assert_eq!(payload.state().transmitted_beacons, 2);
    assert_eq!(payload.state().beacon_send_errors, 0);
    assert_eq!(payload.state().dropped_notifications, 0);
}

#[test]
fn sends_completing_at_reconfigured_deadline_do_not_skip_due_beacons() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    beacon_trace.blocked.set(true);
    let replacement = replacement_config();
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(10_000_000);
        drive(future.as_mut());
        now.set(12_000_000);
        let command = PayloadCommand::ConfigureBeacon(replacement);
        commands.try_send(command).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(command))
        );
        now.set(14_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        assert_eq!(beacon_trace.completed.get(), 0);
        now.set(15_000_000);
        beacon_trace.blocked.set(false);
        drive(future.as_mut());
        assert_beacon(&beacon_trace, 0, config(), 10_000);
        assert_tick(&beacon_trace, 1, replacement, 15_000);
        assert_eq!(events.try_receive(), Ok(notification(config(), false)));
        assert_eq!(events.try_receive(), Ok(notification(replacement, false)));
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 2);
        assert!(events.try_receive().is_err());
    }
    assert_eq!(payload.next_beacon_at, Some(18_000_000));
    assert_eq!(payload.state().transmitted_beacons, 2);
    assert_eq!(payload.state().beacon_send_errors, 0);
    assert_eq!(payload.state().dropped_notifications, 0);
}

#[test]
fn reconfiguring_during_stalled_sends_keeps_old_config_and_rebases_deadline() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    beacon_trace.blocked.set(true);
    let replacement = replacement_config();
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(10_000_000);
        drive(future.as_mut());
        now.set(12_000_000);
        let command = PayloadCommand::ConfigureBeacon(replacement);
        commands.try_send(command).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(command))
        );
        assert_attempt_count(&beacon_trace, 1);
        assert_eq!(beacon_trace.completed.get(), 0);
        now.set(14_000_000);
        beacon_trace.blocked.set(false);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_notifications(events.dyn_receiver(), config(), false);
        now.set(14_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        assert!(events.try_receive().is_err());
        now.set(15_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 1, replacement, 15_000);
        assert_notifications(events.dyn_receiver(), replacement, false);
    }
    assert_eq!(payload.state().beacon_config, replacement);
    assert_eq!(payload.state().transmitted_beacons, 2);
    assert_eq!(payload.next_beacon_at, Some(18_000_000));
    assert_eq!(payload.state().dropped_notifications, 0);
}

#[test]
fn stalled_send_services_commands_and_receiver_without_recreating_send_future() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    beacon_trace.blocked.set(true);
    let replacement = replacement_config();
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(10_000_000);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        assert_eq!(beacon_trace.completed.get(), 0);
        assert!(events.try_receive().is_err());
        for (seconds, command, response) in [
            (11, PayloadCommand::Ping, PayloadEvent::Pong),
            (
                12,
                PayloadCommand::ConfigureBeacon(replacement),
                PayloadEvent::CommandCompleted(PayloadCommand::ConfigureBeacon(replacement)),
            ),
            (13, PayloadCommand::Ping, PayloadEvent::Pong),
            (
                14,
                PayloadCommand::GetState,
                PayloadEvent::State(PayloadState::new(replacement)),
            ),
        ] {
            now.set(seconds * 1_000_000);
            commands.try_send(command).unwrap();
            drive(future.as_mut());
            assert_eq!(events.try_receive(), Ok(response));
            assert_eq!(commands.len(), 0);
            assert_attempt_count(&beacon_trace, 1);
            assert_eq!(beacon_trace.completed.get(), 0);
        }
        for (seconds, response) in [
            (15, stored(client_source(), 11)),
            (18, stored(client_source(), 11)),
            (
                21,
                rejected(
                    client_source(),
                    11,
                    ClientRecordRejection::Storage(QueueErrorKind::Full),
                ),
            ),
        ] {
            now.set(seconds * 1_000_000);
            receive.packet(client_source(), b"independent");
            drive(future.as_mut());
            assert_eq!(events.try_receive(), Ok(response));
            assert_attempt_count(&beacon_trace, 1);
            assert_eq!(beacon_trace.completed.get(), 0);
        }
        now.set(22_000_000);
        commands.try_send(PayloadCommand::GetState).unwrap();
        drive(future.as_mut());
        assert_eq!(
            state_event(events.dyn_receiver()),
            PayloadState {
                reception_enabled: true,
                received_datagrams: 3,
                stored_records: 2,
                rejected_records: 1,
                storage_errors: 1,
                last_client: Some(client_source()),
                ..PayloadState::new(replacement)
            }
        );
        beacon_trace.blocked.set(false);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_notifications(events.dyn_receiver(), config(), false);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        now.set(24_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        assert!(events.try_receive().is_err());
        now.set(25_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 1, replacement, 25_000);
        assert_notifications(events.dyn_receiver(), replacement, false);
    }
    assert_eq!(
        *payload.state(),
        PayloadState {
            reception_enabled: true,
            transmitted_beacons: 2,
            received_datagrams: 3,
            stored_records: 2,
            rejected_records: 1,
            storage_errors: 1,
            last_client: Some(client_source()),
            ..PayloadState::new(replacement)
        }
    );
    assert_eq!(payload.next_beacon_at, Some(28_000_000));
    assert_client_record(
        &mut payload.storage,
        client_source(),
        15_000,
        b"independent",
    );
    assert_client_record(
        &mut payload.storage,
        client_source(),
        18_000,
        b"independent",
    );
}

#[test]
fn process_next_command_uses_command_time_deadlines_across_early_and_late_resume() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
    }
    assert_eq!(payload.next_beacon_at, Some(10_000_000));
    let replacement = replacement_config();
    let command = PayloadCommand::ConfigureBeacon(replacement);
    now.set(5_000_000);
    commands.try_send(command).unwrap();
    commands.try_send(PayloadCommand::GetState).unwrap();
    assert_eq!(
        run_ready(payload.process_next_command_with_clock(&timer)),
        PayloadEvent::CommandCompleted(command)
    );
    assert_eq!(payload.state().beacon_config, replacement);
    assert_eq!(payload.next_beacon_at, Some(8_000_000));
    assert_eq!(commands.len(), 1);
    assert_eq!(
        run_ready(payload.process_next_command_with_clock(&timer)),
        PayloadEvent::State(PayloadState::new(replacement))
    );
    assert_eq!(payload.pending_event, None);
    assert!(events.try_receive().is_err());
    now.set(7_000_000);
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 0);
        assert!(events.try_receive().is_err());
    }
    assert_eq!(payload.next_beacon_at, Some(8_000_000));
    now.set(20_000_000);
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, replacement, 20_000);
        assert_notifications(events.dyn_receiver(), replacement, false);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        assert!(events.try_receive().is_err());
        now.set(22_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        now.set(23_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 1, replacement, 23_000);
        assert_notifications(events.dyn_receiver(), replacement, false);
    }
    assert_eq!(payload.next_beacon_at, Some(26_000_000));
}

#[test]
fn overdue_ticks_send_once_without_catch_up_bursts() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(105_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 105_000);
        assert_notifications(events.dyn_receiver(), config(), false);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        now.set(114_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        now.set(115_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 1, config(), 115_000);
        assert_notifications(events.dyn_receiver(), config(), false);
    }
    assert_eq!(payload.next_beacon_at, Some(125_000_000));
    assert_eq!(payload.state().transmitted_beacons, 2);
}

#[test]
fn stalled_sends_skip_stale_reconfigured_deadlines_without_a_burst() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, beacon_trace) = setup(events.dyn_sender(), commands.dyn_receiver());
    beacon_trace.blocked.set(true);
    let replacement = replacement_config();
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(10_000_000);
        drive(future.as_mut());
        now.set(12_000_000);
        let command = PayloadCommand::ConfigureBeacon(replacement);
        commands.try_send(command).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(command))
        );
        now.set(30_000_000);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        assert_eq!(beacon_trace.completed.get(), 0);
        beacon_trace.blocked.set(false);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_notifications(events.dyn_receiver(), config(), false);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        now.set(32_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        now.set(33_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 1, replacement, 33_000);
        assert_notifications(events.dyn_receiver(), replacement, false);
    }
    assert_eq!(payload.next_beacon_at, Some(36_000_000));
    assert_eq!(payload.state().transmitted_beacons, 2);
    assert_eq!(payload.state().dropped_notifications, 0);
}

struct QueuedPacket {
    source: Endpoint,
    payload: Vec<u8>,
    metadata_length: usize,
}

#[derive(Default)]
struct ReceiveTrace {
    queued: RefCell<VecDeque<Result<QueuedPacket, ()>>>,
    buffer_sizes: RefCell<Vec<usize>>,
    started: Cell<usize>,
    completed: Cell<usize>,
    polled: Cell<usize>,
    cancelled: Cell<usize>,
    discarded: Cell<usize>,
    blocked: Cell<bool>,
}

impl ReceiveTrace {
    fn packet(&self, source: Endpoint, payload: &[u8]) {
        self.metadata(source, payload, payload.len());
    }

    fn metadata(&self, source: Endpoint, payload: &[u8], metadata_length: usize) {
        self.queued.borrow_mut().push_back(Ok(QueuedPacket {
            source,
            payload: payload.to_vec(),
            metadata_length,
        }));
    }

    fn fail(&self) {
        self.queued.borrow_mut().push_back(Err(()));
    }
}

struct ReceiveAttempt {
    trace: Rc<ReceiveTrace>,
    completed: bool,
}

impl Drop for ReceiveAttempt {
    fn drop(&mut self) {
        if !self.completed {
            self.trace.cancelled.set(self.trace.cancelled.get() + 1);
        }
    }
}

struct QueuedClientSocket(Rc<ReceiveTrace>);

impl DatagramSocket for QueuedClientSocket {
    type Error = ();

    async fn send_to(
        &mut self,
        _destination: Endpoint,
        _payload: &[u8],
    ) -> Result<(), Self::Error> {
        panic!("unexpected client send");
    }

    fn recv_from(
        &mut self,
        buffer: &mut [u8],
    ) -> impl Future<Output = Result<DatagramMetadata, Self::Error>> {
        self.0.started.set(self.0.started.get() + 1);
        self.0.buffer_sizes.borrow_mut().push(buffer.len());
        let mut attempt = ReceiveAttempt {
            trace: self.0.clone(),
            completed: false,
        };
        async move {
            poll_fn(|_| {
                attempt.trace.polled.set(attempt.trace.polled.get() + 1);
                if attempt.trace.blocked.get() {
                    return Poll::Pending;
                }
                let Some(result) = attempt.trace.queued.borrow_mut().pop_front() else {
                    return Poll::Pending;
                };
                attempt.completed = true;
                attempt
                    .trace
                    .completed
                    .set(attempt.trace.completed.get() + 1);
                Poll::Ready(result.map(|packet| {
                    let copied = packet.payload.len().min(buffer.len());
                    buffer[..copied].copy_from_slice(&packet.payload[..copied]);
                    DatagramMetadata::new(packet.source, packet.metadata_length)
                }))
            })
            .await
        }
    }

    fn discard_oversized(&mut self, _capacity: usize) -> Option<DatagramMetadata> {
        self.0.discarded.set(self.0.discarded.get() + 1);
        None
    }
}

#[derive(Default)]
struct StorageTrace {
    attempts: RefCell<Vec<Vec<u8>>>,
    completed: Cell<usize>,
    blocked: Cell<bool>,
    failures: RefCell<VecDeque<QueueErrorKind>>,
    maximum: Cell<Option<usize>>,
}

struct TracedStorage {
    inner: RamStorage<272, 2>,
    trace: Rc<StorageTrace>,
}

impl TracedStorage {
    fn new(trace: Rc<StorageTrace>) -> Self {
        Self {
            inner: RamStorage::new(),
            trace,
        }
    }
}

impl QueueStorage for TracedStorage {
    type Error = QueueErrorKind;

    fn max_entry_size(&self) -> Option<usize> {
        self.trace.maximum.get()
    }

    fn error_kind(error: &Self::Error) -> QueueErrorKind {
        *error
    }

    fn append(&mut self, entry: &[u8]) -> impl Future<Output = Result<(), Self::Error>> {
        self.trace.attempts.borrow_mut().push(entry.to_vec());
        async move {
            poll_fn(|_| {
                if self.trace.blocked.get() {
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;
            self.trace.completed.set(self.trace.completed.get() + 1);
            if let Some(error) = self.trace.failures.borrow_mut().pop_front() {
                return Err(error);
            }
            if self
                .trace
                .maximum
                .get()
                .is_some_and(|maximum| entry.len() > maximum)
            {
                return Err(QueueErrorKind::EntryTooLong);
            }
            self.inner
                .append(entry)
                .await
                .map_err(|error| RamStorage::<272, 2>::error_kind(&error))
        }
    }

    async fn peek(&mut self, buffer: &mut [u8]) -> Result<Option<QueueMetadata>, Self::Error> {
        self.inner
            .peek(buffer)
            .await
            .map_err(|error| RamStorage::<272, 2>::error_kind(&error))
    }

    async fn pop(&mut self) -> Result<bool, Self::Error> {
        self.inner
            .pop()
            .await
            .map_err(|error| RamStorage::<272, 2>::error_kind(&error))
    }

    async fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner
            .clear()
            .await
            .map_err(|error| RamStorage::<272, 2>::error_kind(&error))
    }
}

type ReceivingPayload<'a, S> = PayloadSystem<'a, BeaconSocket, QueuedClientSocket, S>;

fn setup_receiving<'a, S: QueueStorage>(
    events: DynamicSender<'a, PayloadEvent>,
    commands: DynamicReceiver<'a, PayloadCommand>,
    storage: S,
) -> (ReceivingPayload<'a, S>, Rc<SocketTrace>, Rc<ReceiveTrace>) {
    let beacon_trace = Rc::new(SocketTrace::default());
    let receive = Rc::new(ReceiveTrace::default());
    let payload = PayloadSystem::new(
        BeaconSocket(beacon_trace.clone()),
        QueuedClientSocket(receive.clone()),
        storage,
        events,
        commands,
        config(),
    );
    (payload, beacon_trace, receive)
}

fn client_source() -> Endpoint {
    Endpoint::new(Address::new(Address::MAX_VALUE).unwrap(), u8::MAX)
}

fn stored(source: Endpoint, payload_length: usize) -> PayloadEvent {
    PayloadEvent::ClientRecordStored {
        source,
        payload_length,
    }
}

fn rejected(
    source: Endpoint,
    payload_length: usize,
    reason: ClientRecordRejection,
) -> PayloadEvent {
    PayloadEvent::ClientRecordRejected {
        source,
        payload_length,
        reason,
    }
}

fn state_event(events: DynamicReceiver<'_, PayloadEvent>) -> PayloadState {
    match events.try_receive().unwrap() {
        PayloadEvent::State(state) => state,
        other => panic!("expected state snapshot, got {other:?}"),
    }
}

fn assert_client_record<S: QueueStorage>(
    storage: &mut S,
    source: Endpoint,
    received_at_ms: u64,
    payload: &[u8],
) {
    let mut buffer = [0; 272];
    let metadata = run_ready(storage.peek(&mut buffer)).unwrap().unwrap();
    assert_eq!(metadata.length(), 16 + payload.len());
    let record = client_record_codec::decode(&buffer[..metadata.length()]).unwrap();
    assert_eq!(record.source, source);
    assert_eq!(record.received_at_ms, received_at_ms);
    assert_eq!(record.payload, payload);
    assert!(run_ready(storage.pop()).unwrap());
}

#[test]
fn constructor_initializes_all_client_counters_and_last_client() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
    let (payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    let state = payload.state();
    assert!(state.reception_enabled);
    assert_eq!(state.received_datagrams, 0);
    assert_eq!(state.stored_records, 0);
    assert_eq!(state.rejected_records, 0);
    assert_eq!(state.receive_errors, 0);
    assert_eq!(state.storage_errors, 0);
    assert_eq!(state.last_client, None);
    assert_eq!(receive.started.get(), 0);
}

#[test]
fn client_binary_and_empty_payloads_preserve_valid_source_and_clock_time() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    let binary = [0, 255, 128, 1, 0, 13, 10];
    let empty_source = Endpoint::new(Address::new_unicast(Address::MAX_VALUE - 1).unwrap(), 254);
    receive.packet(client_source(), &binary);
    receive.packet(empty_source, &[]);
    let now = Rc::new(Cell::new(0x0102_0304_0506_0708));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
    }
    assert_eq!(
        events.try_receive(),
        Ok(stored(client_source(), binary.len()))
    );
    assert_eq!(events.try_receive(), Ok(stored(empty_source, 0)));
    assert!(events.try_receive().is_err());
    assert_eq!(
        *payload.state(),
        PayloadState {
            reception_enabled: true,
            received_datagrams: 2,
            stored_records: 2,
            last_client: Some(empty_source),
            ..PayloadState::new(config())
        }
    );
    assert_client_record(
        &mut payload.storage,
        client_source(),
        now.get() / 1_000,
        &binary,
    );
    assert_client_record(&mut payload.storage, empty_source, now.get() / 1_000, &[]);
    assert!(payload.storage.is_empty());
}

#[test]
fn client_maximum_payload_uses_exact_receive_and_record_limits() {
    assert_eq!(MAX_CLIENT_PAYLOAD_SIZE, 256);
    assert_eq!(MAX_CLIENT_RECORD_SIZE, 272);
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let trace = Rc::new(StorageTrace::default());
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        TracedStorage::new(trace.clone()),
    );
    let bytes: Vec<u8> = (0..=255).collect();
    receive.packet(client_source(), &bytes);
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
    }
    assert_eq!(events.try_receive(), Ok(stored(client_source(), 256)));
    assert!(
        receive
            .buffer_sizes
            .borrow()
            .iter()
            .all(|size| *size == 256)
    );
    assert_eq!(trace.attempts.borrow().len(), 1);
    assert_eq!(trace.attempts.borrow()[0].len(), 272);
    assert_eq!(payload.state().stored_records, 1);
    assert_client_record(&mut payload.storage, client_source(), 0, &bytes);
}

#[test]
fn client_queue_is_fifo_and_full_rejects_without_overwriting_or_changing_last_client() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    let sources = [
        client_source(),
        Endpoint::new(Address::new(42).unwrap(), 8),
        Endpoint::new(Address::new(43).unwrap(), 9),
    ];
    for (source, bytes) in sources
        .into_iter()
        .zip([b"first".as_slice(), b"second", b"third"])
    {
        receive.packet(source, bytes);
    }
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
    }
    assert_eq!(events.try_receive(), Ok(stored(sources[0], 5)));
    assert_eq!(events.try_receive(), Ok(stored(sources[1], 6)));
    assert_eq!(
        events.try_receive(),
        Ok(rejected(
            sources[2],
            5,
            ClientRecordRejection::Storage(QueueErrorKind::Full)
        ))
    );
    assert!(events.try_receive().is_err());
    assert_eq!(
        *payload.state(),
        PayloadState {
            reception_enabled: true,
            received_datagrams: 3,
            stored_records: 2,
            rejected_records: 1,
            storage_errors: 1,
            last_client: Some(sources[1]),
            ..PayloadState::new(config())
        }
    );
    assert_client_record(&mut payload.storage, sources[0], 0, b"first");
    assert_client_record(&mut payload.storage, sources[1], 0, b"second");
    assert!(payload.storage.is_empty());
}

#[test]
fn client_small_advertised_storage_limit_still_attempts_append_and_classifies_error() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let trace = Rc::new(StorageTrace::default());
    trace.maximum.set(Some(15));
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        TracedStorage::new(trace.clone()),
    );
    receive.packet(client_source(), &[]);
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
    }
    assert_eq!(trace.attempts.borrow().len(), 1);
    assert_eq!(trace.completed.get(), 1);
    assert_eq!(
        events.try_receive(),
        Ok(rejected(
            client_source(),
            0,
            ClientRecordRejection::Storage(QueueErrorKind::EntryTooLong)
        ))
    );
    assert_eq!(
        *payload.state(),
        PayloadState {
            reception_enabled: true,
            received_datagrams: 1,
            rejected_records: 1,
            storage_errors: 1,
            ..PayloadState::new(config())
        }
    );
    assert!(payload.storage.inner.is_empty());
}

#[test]
fn client_append_errors_use_storage_error_kind_and_receive_next_record() {
    for kind in [
        QueueErrorKind::Full,
        QueueErrorKind::EntryTooLong,
        QueueErrorKind::BufferTooSmall,
        QueueErrorKind::CorruptEntry,
        QueueErrorKind::Media,
        QueueErrorKind::Other,
    ] {
        let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
        let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
        let trace = Rc::new(StorageTrace::default());
        trace.failures.borrow_mut().push_back(kind);
        let (mut payload, _, receive) = setup_receiving(
            events.dyn_sender(),
            commands.dyn_receiver(),
            TracedStorage::new(trace.clone()),
        );
        receive.packet(Endpoint::new(Address::new(1).unwrap(), 2), b"failed");
        receive.packet(client_source(), b"recovered");
        let mut timer = FakeTimer(Rc::new(Cell::new(0)));
        {
            let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
            drive(future.as_mut());
        }
        assert_eq!(
            events.try_receive(),
            Ok(rejected(
                Endpoint::new(Address::new(1).unwrap(), 2),
                6,
                ClientRecordRejection::Storage(kind)
            ))
        );
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 9)));
        assert!(events.try_receive().is_err());
        assert_eq!(trace.completed.get(), 2);
        assert_eq!(
            *payload.state(),
            PayloadState {
                reception_enabled: true,
                received_datagrams: 2,
                stored_records: 1,
                rejected_records: 1,
                storage_errors: 1,
                last_client: Some(client_source()),
                ..PayloadState::new(config())
            }
        );
        assert_client_record(&mut payload.storage, client_source(), 0, b"recovered");
    }
}

#[test]
fn client_invalid_metadata_is_counted_rejected_without_slicing_or_appending() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let trace = Rc::new(StorageTrace::default());
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        TracedStorage::new(trace.clone()),
    );
    for length in [257, usize::MAX] {
        receive.metadata(client_source(), &[], length);
    }
    let good_source = Endpoint::new(Address::new(1).unwrap(), 2);
    receive.packet(good_source, b"valid");
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
    }
    for length in [257, usize::MAX] {
        assert_eq!(
            events.try_receive(),
            Ok(rejected(
                client_source(),
                length,
                ClientRecordRejection::PayloadTooLong
            ))
        );
    }
    assert_eq!(events.try_receive(), Ok(stored(good_source, 5)));
    assert_eq!(trace.attempts.borrow().len(), 1);
    assert_eq!(
        *payload.state(),
        PayloadState {
            reception_enabled: true,
            received_datagrams: 3,
            stored_records: 1,
            rejected_records: 2,
            last_client: Some(good_source),
            ..PayloadState::new(config())
        }
    );
    assert_client_record(&mut payload.storage, good_source, 0, b"valid");
}

#[test]
fn client_receive_error_waits_one_hundred_ms_without_blocking_commands_or_beacon() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 8>::new();
    let (mut payload, beacon_trace, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(9_950_000);
        receive.fail();
        receive.packet(client_source(), b"recovered");
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::ClientReceiveFailed));
        assert_eq!(receive.completed.get(), 1);
        let attempts_after_error = receive.started.get();
        for micros in [9_950_000, 10_000_000, 10_049_999] {
            now.set(micros);
            commands.try_send(PayloadCommand::Ping).unwrap();
            drive(future.as_mut());
            assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
            if micros == 10_000_000 {
                assert_tick(&beacon_trace, 0, config(), 10_000);
                assert_notifications(events.dyn_receiver(), config(), false);
            }
            assert_eq!(receive.started.get(), attempts_after_error);
            assert_eq!(receive.completed.get(), 1);
            assert_eq!(receive.queued.borrow().len(), 1);
        }
        now.set(10_050_000);
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 9)));
    }
    assert_eq!(
        *payload.state(),
        PayloadState {
            reception_enabled: true,
            transmitted_beacons: 1,
            received_datagrams: 1,
            stored_records: 1,
            receive_errors: 1,
            last_client: Some(client_source()),
            ..PayloadState::new(config())
        }
    );
    assert_client_record(&mut payload.storage, client_source(), 10_050, b"recovered");
}

#[test]
fn client_repeated_receive_errors_each_require_a_new_backoff() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    receive.fail();
    receive.fail();
    receive.packet(client_source(), &[]);
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        for (micros, completed) in [
            (0, 1),
            (99_999, 1),
            (100_000, 2),
            (199_999, 2),
            (200_000, 3),
        ] {
            now.set(micros);
            drive(future.as_mut());
            assert_eq!(receive.completed.get(), completed);
        }
    }
    assert_eq!(events.try_receive(), Ok(PayloadEvent::ClientReceiveFailed));
    assert_eq!(events.try_receive(), Ok(PayloadEvent::ClientReceiveFailed));
    assert_eq!(events.try_receive(), Ok(stored(client_source(), 0)));
    assert_eq!(payload.state().receive_errors, 2);
    assert_eq!(payload.state().received_datagrams, 1);
    assert_eq!(payload.state().stored_records, 1);
    assert_eq!(payload.state().rejected_records, 0);
    assert_eq!(payload.state().storage_errors, 0);
}

#[test]
fn pending_client_append_keeps_timestamp_services_beacon_and_commands_without_restart() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 8>::new();
    let trace = Rc::new(StorageTrace::default());
    trace.blocked.set(true);
    let (mut payload, beacon_trace, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        TracedStorage::new(trace.clone()),
    );
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    let second_source = Endpoint::new(Address::new(123).unwrap(), 17);
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(1_234_567);
        receive.packet(client_source(), b"blocked");
        receive.packet(second_source, b"next");
        drive(future.as_mut());
        assert_eq!(trace.attempts.borrow().len(), 1);
        assert_eq!(trace.completed.get(), 0);
        assert_eq!(receive.completed.get(), 1);
        let receive_attempts = receive.started.get();
        assert!(events.try_receive().is_err());
        for (micros, command) in [
            (2_000_000, PayloadCommand::Ping),
            (3_000_000, PayloadCommand::GetState),
            (4_000_000, PayloadCommand::ConfigureBeacon(config())),
        ] {
            now.set(micros);
            commands.try_send(command).unwrap();
            drive(future.as_mut());
            match command {
                PayloadCommand::Ping => assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong)),
                PayloadCommand::GetState => assert_eq!(
                    state_event(events.dyn_receiver()),
                    PayloadState {
                        reception_enabled: true,
                        received_datagrams: 1,
                        ..PayloadState::new(config())
                    }
                ),
                _ => assert_eq!(
                    events.try_receive(),
                    Ok(PayloadEvent::CommandCompleted(command))
                ),
            }
            assert_eq!(trace.attempts.borrow().len(), 1);
            assert_eq!(trace.completed.get(), 0);
            assert_eq!(receive.started.get(), receive_attempts);
            assert_eq!(receive.queued.borrow().len(), 1);
        }
        now.set(14_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 14_000);
        assert_notifications(events.dyn_receiver(), config(), false);
        assert_eq!(trace.attempts.borrow().len(), 1);
        assert_eq!(trace.completed.get(), 0);
        assert_eq!(receive.completed.get(), 1);
        trace.blocked.set(false);
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 7)));
        assert_eq!(events.try_receive(), Ok(stored(second_source, 4)));
        assert!(events.try_receive().is_err());
    }
    assert_eq!(trace.attempts.borrow().len(), 2);
    assert_eq!(trace.completed.get(), 2);
    assert_eq!(
        *payload.state(),
        PayloadState {
            reception_enabled: true,
            transmitted_beacons: 1,
            received_datagrams: 2,
            stored_records: 2,
            last_client: Some(second_source),
            ..PayloadState::new(config())
        }
    );
    assert_client_record(&mut payload.storage, client_source(), 1_234, b"blocked");
    assert_client_record(&mut payload.storage, second_source, 14_000, b"next");
}

#[test]
fn client_worker_yields_after_each_immediately_ready_record() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let trace = Rc::new(StorageTrace::default());
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        TracedStorage::new(trace.clone()),
    );
    for _ in 0..3 {
        receive.packet(client_source(), &[]);
    }
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(receive.completed.get(), 1);
    assert_eq!(trace.completed.get(), 1);
    assert_eq!(receive.queued.borrow().len(), 2);
    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(receive.completed.get(), 2);
    assert_eq!(trace.completed.get(), 2);
    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(receive.completed.get(), 3);
    assert_eq!(trace.completed.get(), 3);
    assert_eq!(events.try_receive(), Ok(stored(client_source(), 0)));
    assert_eq!(events.try_receive(), Ok(stored(client_source(), 0)));
    assert_eq!(
        events.try_receive(),
        Ok(rejected(
            client_source(),
            0,
            ClientRecordRejection::Storage(QueueErrorKind::Full)
        ))
    );
}

#[test]
fn full_events_drop_client_notifications_but_keep_storage_beacon_and_reliable_pong() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
    let trace = Rc::new(StorageTrace::default());
    let (mut payload, beacon_trace, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        TracedStorage::new(trace.clone()),
    );
    let filler = PayloadEvent::State(PayloadState::new(config()));
    events.try_send(filler).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();
    receive.packet(client_source(), b"stored");
    receive.metadata(client_source(), &[], 257);
    receive.fail();
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_eq!(trace.completed.get(), 1);
        assert_eq!(receive.completed.get(), 3);
        trace.failures.borrow_mut().push_back(QueueErrorKind::Media);
        receive.packet(Endpoint::new(Address::new(7).unwrap(), 3), b"media failure");
        now.set(10_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_eq!(events.try_receive(), Ok(filler));
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        drive(future.as_mut());
        assert!(events.try_receive().is_err());
    }
    assert_eq!(payload.pending_event, None);
    assert_eq!(
        *payload.state(),
        PayloadState {
            reception_enabled: true,
            transmitted_beacons: 1,
            dropped_notifications: 5,
            received_datagrams: 3,
            stored_records: 1,
            rejected_records: 2,
            receive_errors: 1,
            storage_errors: 1,
            last_client: Some(client_source()),
            ..PayloadState::new(config())
        }
    );
    assert_client_record(&mut payload.storage, client_source(), 0, b"stored");
}

#[test]
fn retained_reliable_reply_precedes_best_effort_client_notification() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    payload.pending_event = Some(PayloadEvent::Pong);
    receive.packet(client_source(), &[]);
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        drive(future.as_mut());
        assert!(events.try_receive().is_err());
    }
    assert_eq!(payload.state().stored_records, 1);
    assert_eq!(payload.state().dropped_notifications, 1);
}

#[test]
fn pending_receive_is_not_restarted_by_commands_and_cancelled_run_keeps_queued_datagram() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    receive.blocked.set(true);
    receive.packet(client_source(), b"retained");
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        let attempts = receive.started.get();
        assert_eq!(attempts, 1);
        for command in [
            PayloadCommand::Ping,
            PayloadCommand::GetState,
            PayloadCommand::ConfigureBeacon(replacement_config()),
            PayloadCommand::SetReceptionEnabled(true),
            PayloadCommand::SetReceptionEnabled(true),
        ] {
            commands.try_send(command).unwrap();
            drive(future.as_mut());
            let response = match command {
                PayloadCommand::Ping => PayloadEvent::Pong,
                PayloadCommand::GetState => PayloadEvent::State(PayloadState::new(config())),
                _ => PayloadEvent::CommandCompleted(command),
            };
            assert_eq!(events.try_receive(), Ok(response));
            assert_eq!(receive.started.get(), attempts);
            assert_eq!(receive.cancelled.get(), 0);
        }
    }
    assert_eq!(receive.completed.get(), 0);
    assert_eq!(receive.queued.borrow().len(), 1);
    assert_eq!(payload.state().received_datagrams, 0);
    receive.blocked.set(false);
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
    }
    assert_eq!(events.try_receive(), Ok(stored(client_source(), 8)));
    assert_eq!(receive.completed.get(), 1);
    assert_eq!(payload.state().received_datagrams, 1);
    assert_eq!(payload.state().stored_records, 1);
    assert_client_record(&mut payload.storage, client_source(), 0, b"retained");
}

#[test]
fn client_failures_preserve_the_last_successfully_stored_source() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let trace = Rc::new(StorageTrace::default());
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        TracedStorage::new(trace.clone()),
    );
    receive.packet(client_source(), b"good");
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 4)));
        let failed_source = Endpoint::new(Address::new(1).unwrap(), 2);
        trace.failures.borrow_mut().push_back(QueueErrorKind::Media);
        receive.packet(failed_source, b"bad");
        receive.metadata(failed_source, &[], 257);
        receive.fail();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(rejected(
                failed_source,
                3,
                ClientRecordRejection::Storage(QueueErrorKind::Media)
            ))
        );
        assert_eq!(
            events.try_receive(),
            Ok(rejected(
                failed_source,
                257,
                ClientRecordRejection::PayloadTooLong
            ))
        );
        assert_eq!(events.try_receive(), Ok(PayloadEvent::ClientReceiveFailed));
    }
    assert_eq!(
        *payload.state(),
        PayloadState {
            reception_enabled: true,
            received_datagrams: 3,
            stored_records: 1,
            rejected_records: 2,
            receive_errors: 1,
            storage_errors: 1,
            last_client: Some(client_source()),
            ..PayloadState::new(config())
        }
    );
    assert_client_record(&mut payload.storage, client_source(), 0, b"good");
}

#[test]
fn stalled_beacon_send_leaves_client_receive_and_storage_running() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, beacon_trace, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    beacon_trace.blocked.set(true);
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(10_000_000);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        assert_eq!(beacon_trace.completed.get(), 0);
        receive.packet(client_source(), b"independent");
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 11)));
        commands.try_send(PayloadCommand::Ping).unwrap();
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        assert_attempt_count(&beacon_trace, 1);
        assert_eq!(beacon_trace.completed.get(), 0);
        beacon_trace.blocked.set(false);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_notifications(events.dyn_receiver(), config(), false);
    }
    assert_eq!(payload.state().received_datagrams, 1);
    assert_eq!(payload.state().stored_records, 1);
    assert_client_record(
        &mut payload.storage,
        client_source(),
        10_000,
        b"independent",
    );
}

#[test]
fn client_worker_yields_after_each_invalid_metadata_record() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let trace = Rc::new(StorageTrace::default());
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        TracedStorage::new(trace.clone()),
    );
    for _ in 0..3 {
        receive.metadata(client_source(), &[], usize::MAX);
    }
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        let mut context = Context::from_waker(Waker::noop());
        for completed in 1..=3 {
            assert!(future.as_mut().poll(&mut context).is_pending());
            assert_eq!(receive.completed.get(), completed);
            assert_eq!(
                events.try_receive(),
                Ok(rejected(
                    client_source(),
                    usize::MAX,
                    ClientRecordRejection::PayloadTooLong
                ))
            );
        }
    }
    assert!(trace.attempts.borrow().is_empty());
    assert_eq!(payload.state().received_datagrams, 3);
    assert_eq!(payload.state().rejected_records, 3);
    assert_eq!(payload.state().stored_records, 0);
    assert_eq!(payload.state().storage_errors, 0);
    assert_eq!(payload.state().last_client, None);
}

#[test]
fn direct_reception_commands_apply_before_run_and_report_the_current_mode() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    receive.packet(client_source(), b"retained");
    assert!(payload.state().reception_enabled);
    for enabled in [false, false, true, true, false] {
        let command = PayloadCommand::SetReceptionEnabled(enabled);
        commands.try_send(command).unwrap();
        assert_eq!(
            run_ready(payload.process_next_command()),
            PayloadEvent::CommandCompleted(command)
        );
        assert_eq!(payload.state().reception_enabled, enabled);
        commands.try_send(PayloadCommand::GetState).unwrap();
        assert_eq!(
            run_ready(payload.process_next_command()),
            PayloadEvent::State(*payload.state())
        );
        assert_eq!(receive.started.get(), 0);
        assert_eq!(receive.queued.borrow().len(), 1);
        assert!(events.try_receive().is_err());
    }
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        commands.try_send(PayloadCommand::Ping).unwrap();
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        commands.try_send(PayloadCommand::GetState).unwrap();
        drive(future.as_mut());
        assert_eq!(
            state_event(events.dyn_receiver()),
            PayloadState {
                reception_enabled: false,
                ..PayloadState::new(config())
            }
        );
        assert_eq!(receive.started.get(), 0);
        assert_eq!(receive.polled.get(), 0);
        assert_eq!(receive.discarded.get(), 0);
    }
    assert!(!payload.state().reception_enabled);
    assert!(payload.storage.is_empty());
    assert_eq!(receive.queued.borrow().len(), 1);
}

#[test]
fn pause_cancels_pending_receive_without_consuming_or_polling_retained_packets() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    receive.blocked.set(true);
    receive.packet(client_source(), b"retained");
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_eq!(receive.started.get(), 1);
        assert!(receive.polled.get() > 0);
        let pause = PayloadCommand::SetReceptionEnabled(false);
        commands.try_send(pause).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(pause))
        );
        assert_eq!(receive.cancelled.get(), 1);
        let polls = receive.polled.get();
        receive.blocked.set(false);
        for micros in [1, 100_000, 1_000_000] {
            now.set(micros);
            drive(future.as_mut());
            assert_eq!(receive.started.get(), 1);
            assert_eq!(receive.polled.get(), polls);
            assert_eq!(receive.completed.get(), 0);
            assert_eq!(receive.discarded.get(), 0);
            assert_eq!(receive.queued.borrow().len(), 1);
        }
        commands.try_send(PayloadCommand::GetState).unwrap();
        drive(future.as_mut());
        assert_eq!(
            state_event(events.dyn_receiver()),
            PayloadState {
                reception_enabled: false,
                ..PayloadState::new(config())
            }
        );
        now.set(2_345_678);
        let resume = PayloadCommand::SetReceptionEnabled(true);
        commands.try_send(resume).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(resume))
        );
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 8)));
        assert_eq!(receive.completed.get(), 1);
        assert!(receive.queued.borrow().is_empty());
        assert!(events.try_receive().is_err());
    }
    assert!(payload.state().reception_enabled);
    assert_eq!(payload.state().received_datagrams, 1);
    assert_client_record(&mut payload.storage, client_source(), 2_345, b"retained");
}

#[test]
fn paused_transport_packets_resume_in_fifo_order_with_the_resumed_read_timestamp() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    let pause = PayloadCommand::SetReceptionEnabled(false);
    commands.try_send(pause).unwrap();
    assert_eq!(
        run_ready(payload.process_next_command()),
        PayloadEvent::CommandCompleted(pause)
    );
    let second_source = Endpoint::new(Address::new(123).unwrap(), 17);
    let now = Rc::new(Cell::new(123_456));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        receive.packet(client_source(), b"first");
        drive(future.as_mut());
        now.set(2_345_678);
        receive.packet(second_source, b"second");
        drive(future.as_mut());
        assert_eq!(receive.started.get(), 0);
        assert_eq!(receive.polled.get(), 0);
        assert_eq!(receive.discarded.get(), 0);
        assert_eq!(receive.queued.borrow().len(), 2);
        now.set(7_654_321);
        let resume = PayloadCommand::SetReceptionEnabled(true);
        commands.try_send(resume).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(resume))
        );
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 5)));
        assert_eq!(events.try_receive(), Ok(stored(second_source, 6)));
        assert!(events.try_receive().is_err());
        assert_eq!(receive.completed.get(), 2);
        assert!(receive.queued.borrow().is_empty());
    }
    assert_eq!(payload.state().received_datagrams, 2);
    assert_eq!(payload.state().stored_records, 2);
    assert_eq!(payload.state().last_client, Some(second_source));
    assert_client_record(&mut payload.storage, client_source(), 7_654, b"first");
    assert_client_record(&mut payload.storage, second_source, 7_654, b"second");
    assert!(payload.storage.is_empty());
}

#[test]
fn pause_during_pending_append_finishes_it_once_and_defers_the_next_read_until_resume() {
    for failure in [None, Some(QueueErrorKind::Media)] {
        let commands = Channel::<NoopRawMutex, PayloadCommand, 4>::new();
        let events = Channel::<NoopRawMutex, PayloadEvent, 8>::new();
        let trace = Rc::new(StorageTrace::default());
        trace.blocked.set(true);
        if let Some(error) = failure {
            trace.failures.borrow_mut().push_back(error);
        }
        let (mut payload, _, receive) = setup_receiving(
            events.dyn_sender(),
            commands.dyn_receiver(),
            TracedStorage::new(trace.clone()),
        );
        let second_source = Endpoint::new(Address::new(123).unwrap(), 17);
        receive.packet(client_source(), b"accepted");
        receive.packet(second_source, b"next");
        let now = Rc::new(Cell::new(1_234_567));
        let mut timer = FakeTimer(now.clone());
        {
            let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
            drive(future.as_mut());
            assert_eq!(trace.attempts.borrow().len(), 1);
            assert_eq!(trace.completed.get(), 0);
            assert_eq!(receive.completed.get(), 1);
            let pause = PayloadCommand::SetReceptionEnabled(false);
            commands.try_send(pause).unwrap();
            drive(future.as_mut());
            assert_eq!(
                events.try_receive(),
                Ok(PayloadEvent::CommandCompleted(pause))
            );
            commands.try_send(PayloadCommand::GetState).unwrap();
            drive(future.as_mut());
            assert_eq!(
                state_event(events.dyn_receiver()),
                PayloadState {
                    reception_enabled: false,
                    received_datagrams: 1,
                    ..PayloadState::new(config())
                }
            );
            assert_eq!(trace.completed.get(), 0);
            for enabled in [true, false, true, false] {
                commands
                    .try_send(PayloadCommand::SetReceptionEnabled(enabled))
                    .unwrap();
            }
            drive(future.as_mut());
            for enabled in [true, false, true, false] {
                assert_eq!(
                    events.try_receive(),
                    Ok(PayloadEvent::CommandCompleted(
                        PayloadCommand::SetReceptionEnabled(enabled)
                    ))
                );
            }
            assert_eq!(trace.attempts.borrow().len(), 1);
            assert_eq!(receive.started.get(), 1);
            now.set(4_567_890);
            trace.blocked.set(false);
            drive(future.as_mut());
            let expected = match failure {
                Some(error) => rejected(client_source(), 8, ClientRecordRejection::Storage(error)),
                None => stored(client_source(), 8),
            };
            assert_eq!(events.try_receive(), Ok(expected));
            drive(future.as_mut());
            assert!(events.try_receive().is_err());
            assert_eq!(trace.attempts.borrow().len(), 1);
            assert_eq!(trace.completed.get(), 1);
            assert_eq!(receive.started.get(), 1);
            assert_eq!(receive.completed.get(), 1);
            assert_eq!(receive.queued.borrow().len(), 1);
            let resume = PayloadCommand::SetReceptionEnabled(true);
            commands.try_send(resume).unwrap();
            drive(future.as_mut());
            assert_eq!(
                events.try_receive(),
                Ok(PayloadEvent::CommandCompleted(resume))
            );
            assert_eq!(events.try_receive(), Ok(stored(second_source, 4)));
            assert!(events.try_receive().is_err());
        }
        assert_eq!(trace.attempts.borrow().len(), 2);
        assert_eq!(trace.completed.get(), 2);
        assert_eq!(receive.completed.get(), 2);
        let attempts = trace.attempts.borrow();
        let record = client_record_codec::decode(&attempts[0]).unwrap();
        assert_eq!(record.source, client_source());
        assert_eq!(record.received_at_ms, 1_234);
        assert_eq!(record.payload, b"accepted");
        assert_eq!(payload.state().received_datagrams, 2);
        assert_eq!(
            payload.state().stored_records,
            if failure.is_some() { 1 } else { 2 }
        );
        assert_eq!(payload.state().storage_errors, u64::from(failure.is_some()));
        assert_eq!(
            payload.state().rejected_records,
            u64::from(failure.is_some())
        );
        if failure.is_none() {
            assert_client_record(&mut payload.storage, client_source(), 1_234, b"accepted");
        }
        assert_client_record(&mut payload.storage, second_source, 4_567, b"next");
        assert!(payload.storage.inner.is_empty());
    }
}

#[test]
fn paused_reception_keeps_beacons_ping_state_and_configuration_running() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, beacon_trace, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(3_000_000);
        let pause = PayloadCommand::SetReceptionEnabled(false);
        commands.try_send(pause).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(pause))
        );
        let polls = receive.polled.get();
        receive.packet(client_source(), b"retained");
        now.set(9_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 0);
        now.set(10_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_notifications(events.dyn_receiver(), config(), false);
        commands.try_send(PayloadCommand::Ping).unwrap();
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        now.set(11_000_000);
        let reconfigure = PayloadCommand::ConfigureBeacon(replacement_config());
        commands.try_send(reconfigure).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(reconfigure))
        );
        now.set(13_999_999);
        drive(future.as_mut());
        assert_attempt_count(&beacon_trace, 1);
        beacon_trace.fail.set(true);
        now.set(14_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 1, replacement_config(), 14_000);
        assert_notifications(events.dyn_receiver(), replacement_config(), true);
        commands.try_send(PayloadCommand::GetState).unwrap();
        drive(future.as_mut());
        assert_eq!(
            state_event(events.dyn_receiver()),
            PayloadState {
                reception_enabled: false,
                transmitted_beacons: 1,
                beacon_send_errors: 1,
                ..PayloadState::new(replacement_config())
            }
        );
        assert_eq!(receive.started.get(), 1);
        assert_eq!(receive.polled.get(), polls);
        assert_eq!(receive.cancelled.get(), 1);
        assert_eq!(receive.completed.get(), 0);
        assert_eq!(receive.discarded.get(), 0);
        assert_eq!(receive.queued.borrow().len(), 1);
    }
    assert!(!payload.state().reception_enabled);
    assert_eq!(payload.next_beacon_at, Some(17_000_000));
}

#[test]
fn idempotent_reception_commands_do_not_restart_receive_or_poll_it_while_paused() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 2>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    receive.blocked.set(true);
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
    drive(future.as_mut());
    assert_eq!(receive.started.get(), 1);
    for _ in 0..3 {
        let command = PayloadCommand::SetReceptionEnabled(true);
        commands.try_send(command).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(command))
        );
        assert_eq!(receive.started.get(), 1);
        assert_eq!(receive.cancelled.get(), 0);
    }
    let pause = PayloadCommand::SetReceptionEnabled(false);
    commands.try_send(pause).unwrap();
    drive(future.as_mut());
    assert_eq!(
        events.try_receive(),
        Ok(PayloadEvent::CommandCompleted(pause))
    );
    assert_eq!(receive.cancelled.get(), 1);
    let polls = receive.polled.get();
    for _ in 0..3 {
        commands.try_send(pause).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(pause))
        );
        assert_eq!(receive.started.get(), 1);
        assert_eq!(receive.polled.get(), polls);
        assert_eq!(receive.discarded.get(), 0);
    }
    let resume = PayloadCommand::SetReceptionEnabled(true);
    commands.try_send(resume).unwrap();
    drive(future.as_mut());
    assert_eq!(
        events.try_receive(),
        Ok(PayloadEvent::CommandCompleted(resume))
    );
    assert_eq!(receive.started.get(), 2);
    assert_eq!(receive.cancelled.get(), 1);
    commands.try_send(resume).unwrap();
    drive(future.as_mut());
    assert_eq!(
        events.try_receive(),
        Ok(PayloadEvent::CommandCompleted(resume))
    );
    assert_eq!(receive.started.get(), 2);
    assert_eq!(receive.cancelled.get(), 1);
    assert!(events.try_receive().is_err());
}

#[test]
fn coalesced_reception_toggles_use_latest_applied_state_and_keep_immutable_snapshots() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 8>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 8>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    receive.blocked.set(true);
    let mut timer = FakeTimer(Rc::new(Cell::new(0)));
    let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
    drive(future.as_mut());
    for modes in [[false, true, false], [true, false, true]] {
        for enabled in modes {
            commands
                .try_send(PayloadCommand::SetReceptionEnabled(enabled))
                .unwrap();
            commands.try_send(PayloadCommand::GetState).unwrap();
        }
        drive(future.as_mut());
        for enabled in modes {
            assert_eq!(
                events.try_receive(),
                Ok(PayloadEvent::CommandCompleted(
                    PayloadCommand::SetReceptionEnabled(enabled)
                ))
            );
            assert_eq!(
                state_event(events.dyn_receiver()),
                PayloadState {
                    reception_enabled: enabled,
                    ..PayloadState::new(config())
                }
            );
        }
        assert!(events.try_receive().is_err());
        assert_eq!(
            receive.started.get() - receive.cancelled.get(),
            usize::from(modes[2])
        );
        let attempts = receive.started.get();
        let cancellations = receive.cancelled.get();
        drive(future.as_mut());
        assert_eq!(receive.started.get(), attempts);
        assert_eq!(receive.cancelled.get(), cancellations);
        assert_eq!(receive.completed.get(), 0);
        assert_eq!(receive.discarded.get(), 0);
    }
}

#[test]
fn full_events_allow_pause_to_apply_but_defer_resume_and_drop_only_beacon_notifications() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
    let (mut payload, beacon_trace, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    receive.blocked.set(true);
    receive.packet(client_source(), b"retained");
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    let pause = PayloadCommand::SetReceptionEnabled(false);
    let resume = PayloadCommand::SetReceptionEnabled(true);
    let polls;
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        events.try_send(PayloadEvent::Pong).unwrap();
        commands.try_send(pause).unwrap();
        commands.try_send(resume).unwrap();
        drive(future.as_mut());
        assert_eq!(commands.len(), 1);
        assert_eq!(receive.cancelled.get(), 1);
        polls = receive.polled.get();
        receive.blocked.set(false);
        now.set(10_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 0, config(), 10_000);
        assert_eq!(receive.started.get(), 1);
        assert_eq!(receive.polled.get(), polls);
        assert_eq!(receive.completed.get(), 0);
        assert_eq!(receive.discarded.get(), 0);
        assert_eq!(receive.queued.borrow().len(), 1);
    }
    assert!(!payload.state().reception_enabled);
    assert_eq!(payload.state().dropped_notifications, 1);
    assert_eq!(
        payload.pending_event,
        Some(PayloadEvent::CommandCompleted(pause))
    );
    receive.blocked.set(true);
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(20_000_000);
        drive(future.as_mut());
        assert_tick(&beacon_trace, 1, config(), 20_000);
        assert_eq!(commands.len(), 1);
        assert_eq!(receive.started.get(), 1);
        assert_eq!(receive.polled.get(), polls);
        assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        drive(future.as_mut());
        assert_eq!(commands.len(), 0);
        assert_eq!(receive.started.get(), 2);
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(pause))
        );
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(resume))
        );
        drive(future.as_mut());
        assert!(events.try_receive().is_err());
        receive.blocked.set(false);
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 8)));
        assert!(events.try_receive().is_err());
    }
    assert!(payload.state().reception_enabled);
    assert_eq!(payload.pending_event, None);
    assert_eq!(payload.state().transmitted_beacons, 2);
    assert_eq!(payload.state().dropped_notifications, 2);
    assert_eq!(payload.state().stored_records, 1);
    assert_client_record(&mut payload.storage, client_source(), 20_000, b"retained");
}

#[test]
fn paused_transport_errors_are_not_observed_until_resume_and_then_require_backoff() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    let pause = PayloadCommand::SetReceptionEnabled(false);
    commands.try_send(pause).unwrap();
    assert_eq!(
        run_ready(payload.process_next_command()),
        PayloadEvent::CommandCompleted(pause)
    );
    receive.fail();
    receive.packet(client_source(), b"recovered");
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        now.set(1_000_000);
        commands.try_send(PayloadCommand::GetState).unwrap();
        drive(future.as_mut());
        assert_eq!(
            state_event(events.dyn_receiver()),
            PayloadState {
                reception_enabled: false,
                ..PayloadState::new(config())
            }
        );
        assert_eq!(receive.started.get(), 0);
        assert_eq!(receive.polled.get(), 0);
        assert_eq!(receive.discarded.get(), 0);
        assert_eq!(receive.queued.borrow().len(), 2);
        let resume = PayloadCommand::SetReceptionEnabled(true);
        commands.try_send(resume).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(resume))
        );
        assert_eq!(events.try_receive(), Ok(PayloadEvent::ClientReceiveFailed));
        assert_eq!(receive.started.get(), 1);
        assert_eq!(receive.discarded.get(), 1);
        now.set(1_099_999);
        drive(future.as_mut());
        assert_eq!(receive.started.get(), 1);
        assert_eq!(receive.completed.get(), 1);
        now.set(1_100_000);
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 9)));
        assert!(events.try_receive().is_err());
    }
    assert_eq!(payload.state().receive_errors, 1);
    assert_eq!(payload.state().received_datagrams, 1);
    assert_eq!(payload.state().stored_records, 1);
    assert_client_record(&mut payload.storage, client_source(), 1_100, b"recovered");
}

#[test]
fn reception_toggles_do_not_bypass_or_restart_error_backoff_and_mode_is_checked_after_it() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    receive.fail();
    receive.fail();
    receive.packet(client_source(), b"recovered");
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::ClientReceiveFailed));
        for (micros, enabled) in [
            (10_000, false),
            (20_000, true),
            (40_000, false),
            (99_999, true),
        ] {
            now.set(micros);
            let command = PayloadCommand::SetReceptionEnabled(enabled);
            commands.try_send(command).unwrap();
            drive(future.as_mut());
            assert_eq!(
                events.try_receive(),
                Ok(PayloadEvent::CommandCompleted(command))
            );
            assert_eq!(receive.started.get(), 1);
            assert_eq!(receive.completed.get(), 1);
            assert_eq!(receive.discarded.get(), 1);
            assert_eq!(receive.queued.borrow().len(), 2);
        }
        now.set(100_000);
        drive(future.as_mut());
        assert_eq!(events.try_receive(), Ok(PayloadEvent::ClientReceiveFailed));
        assert_eq!(receive.started.get(), 2);
        assert_eq!(receive.completed.get(), 2);
        now.set(110_000);
        let pause = PayloadCommand::SetReceptionEnabled(false);
        commands.try_send(pause).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(pause))
        );
        let polls = receive.polled.get();
        for micros in [199_999, 200_000, 250_000] {
            now.set(micros);
            drive(future.as_mut());
            assert_eq!(receive.started.get(), 2);
            assert_eq!(receive.polled.get(), polls);
            assert_eq!(receive.discarded.get(), 2);
            assert_eq!(receive.completed.get(), 2);
            assert_eq!(receive.queued.borrow().len(), 1);
        }
        commands.try_send(PayloadCommand::GetState).unwrap();
        drive(future.as_mut());
        assert_eq!(
            state_event(events.dyn_receiver()),
            PayloadState {
                reception_enabled: false,
                receive_errors: 2,
                ..PayloadState::new(config())
            }
        );
        let resume = PayloadCommand::SetReceptionEnabled(true);
        commands.try_send(resume).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(resume))
        );
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 9)));
        assert!(events.try_receive().is_err());
    }
    assert_eq!(payload.state().receive_errors, 2);
    assert_eq!(payload.state().stored_records, 1);
    assert_client_record(&mut payload.storage, client_source(), 250, b"recovered");
}

#[test]
fn cancelled_runs_preserve_reception_mode_without_reusing_stale_transition_signals() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 1>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 4>::new();
    let (mut payload, _, receive) = setup_receiving(
        events.dyn_sender(),
        commands.dyn_receiver(),
        RamStorage::<272, 2>::new(),
    );
    receive.blocked.set(true);
    receive.packet(client_source(), b"retained");
    let now = Rc::new(Cell::new(0));
    let mut timer = FakeTimer(now.clone());
    let pause = PayloadCommand::SetReceptionEnabled(false);
    let resume = PayloadCommand::SetReceptionEnabled(true);
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        commands.try_send(pause).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(pause))
        );
    }
    assert!(!payload.state().reception_enabled);
    assert_eq!(receive.started.get(), 1);
    assert_eq!(receive.cancelled.get(), 1);
    let polls = receive.polled.get();
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_eq!(receive.started.get(), 1);
        assert_eq!(receive.polled.get(), polls);
        commands.try_send(PayloadCommand::GetState).unwrap();
        drive(future.as_mut());
        assert!(!state_event(events.dyn_receiver()).reception_enabled);
        commands.try_send(resume).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(resume))
        );
        assert_eq!(receive.started.get(), 2);
        assert_eq!(receive.cancelled.get(), 1);
    }
    assert!(payload.state().reception_enabled);
    assert_eq!(receive.cancelled.get(), 2);
    commands.try_send(pause).unwrap();
    assert_eq!(
        run_ready(payload.process_next_command()),
        PayloadEvent::CommandCompleted(pause)
    );
    receive.blocked.set(false);
    let polls = receive.polled.get();
    {
        let mut future = core::pin::pin!(payload.run_with_timer(&mut timer));
        drive(future.as_mut());
        assert_eq!(receive.started.get(), 2);
        assert_eq!(receive.polled.get(), polls);
        assert_eq!(receive.completed.get(), 0);
        assert_eq!(receive.discarded.get(), 0);
        assert_eq!(receive.queued.borrow().len(), 1);
        now.set(3_456_789);
        commands.try_send(resume).unwrap();
        drive(future.as_mut());
        assert_eq!(
            events.try_receive(),
            Ok(PayloadEvent::CommandCompleted(resume))
        );
        assert_eq!(events.try_receive(), Ok(stored(client_source(), 8)));
        assert!(events.try_receive().is_err());
    }
    assert!(payload.state().reception_enabled);
    assert_eq!(payload.state().received_datagrams, 1);
    assert_eq!(payload.state().stored_records, 1);
    assert_client_record(&mut payload.storage, client_source(), 3_456, b"retained");
}

#[test]
fn client_record_codec_matches_version_one_big_endian_wire_format() {
    assert_eq!(client_record_codec::HEADER_SIZE, 16);
    let source = Endpoint::new(Address::new(0x2345).unwrap(), u8::MAX);
    let mut buffer = [0xa5; 32];
    let length =
        client_record_codec::encode(source, 0x0123_4567_89ab_cdef, &[0, 255, 128], &mut buffer)
            .unwrap();
    assert_eq!(length, 19);
    assert_eq!(
        &buffer[..length],
        &[
            1, 0, 0, 0x23, 0x45, 255, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0, 3, 0, 255,
            128,
        ]
    );
    assert!(buffer[length..].iter().all(|byte| *byte == 0xa5));
    let record: ClientRecord<'_> = client_record_codec::decode(&buffer[..length]).unwrap();
    assert_eq!(record.source, source);
    assert_eq!(record.received_at_ms, 0x0123_4567_89ab_cdef);
    assert_eq!(record.payload, &[0, 255, 128]);
}

#[test]
fn client_record_codec_roundtrips_valid_boundaries_and_rejects_invalid_source_addresses() {
    for address in [
        Address::new(0).unwrap(),
        Address::new_unicast(Address::MAX_VALUE - 1).unwrap(),
        Address::new(Address::MAX_VALUE).unwrap(),
    ] {
        for port in [0, 63, 64, u8::MAX] {
            for time in [0, u64::MAX] {
                let source = Endpoint::new(address, port);
                let mut buffer = [0; 16];
                assert_eq!(
                    client_record_codec::encode(source, time, &[], &mut buffer),
                    Ok(16)
                );
                assert_eq!(&buffer[1..5], &u32::from(address.get()).to_be_bytes());
                let record = client_record_codec::decode(&buffer).unwrap();
                assert_eq!(record.source, source);
                assert_eq!(record.received_at_ms, time);
                assert!(record.payload.is_empty());
            }
        }
    }
    for raw in [
        u32::from(Address::MAX_VALUE) + 1,
        u32::from(u16::MAX),
        u32::MAX,
    ] {
        let mut buffer = [0; 16];
        assert_eq!(
            client_record_codec::encode(client_source(), u64::MAX, &[], &mut buffer),
            Ok(16)
        );
        buffer[1..5].copy_from_slice(&raw.to_be_bytes());
        assert_eq!(
            client_record_codec::decode(&buffer).err(),
            Some(client_record_codec::Error::InvalidSourceAddress(raw))
        );
    }
}

#[test]
fn client_record_codec_accepts_u16_maximum_payload_without_worker_size_restriction() {
    let bytes: Vec<u8> = (0..usize::from(u16::MAX))
        .map(|value| value as u8)
        .collect();
    let mut buffer = std::vec![0; 16 + bytes.len()];
    assert_eq!(
        client_record_codec::encode(client_source(), u64::MAX, &bytes, &mut buffer),
        Ok(16 + bytes.len())
    );
    assert_eq!(&buffer[14..16], &[255, 255]);
    let record = client_record_codec::decode(&buffer).unwrap();
    assert_eq!(record.source, client_source());
    assert_eq!(record.received_at_ms, u64::MAX);
    assert_eq!(record.payload, bytes.as_slice());
}

#[test]
fn client_record_codec_rejects_payload_length_overflow_and_undersized_output() {
    use client_record_codec::Error;
    let oversized = std::vec![0; usize::from(u16::MAX) + 1];
    let mut buffer = std::vec![0; 16 + oversized.len()];
    assert_eq!(
        client_record_codec::encode(client_source(), 0, &oversized, &mut buffer),
        Err(Error::PayloadTooLong(oversized.len()))
    );
    for (payload, available, required) in
        [(&[][..], 0, 16), (&[][..], 15, 16), (&b"ab"[..], 17, 18)]
    {
        let mut buffer = [0xa5; 18];
        assert_eq!(
            client_record_codec::encode(client_source(), 0, payload, &mut buffer[..available]),
            Err(Error::BufferTooSmall {
                required,
                available
            })
        );
        assert_eq!(buffer, [0xa5; 18]);
    }
}

#[test]
fn client_record_codec_rejects_truncated_header_and_declared_payload() {
    use client_record_codec::Error;
    let mut buffer = [0; 18];
    client_record_codec::encode(client_source(), 0, b"ab", &mut buffer).unwrap();
    for length in 0..18 {
        assert_eq!(
            client_record_codec::decode(&buffer[..length]).err(),
            Some(Error::Truncated)
        );
    }
    buffer[14..16].copy_from_slice(&u16::MAX.to_be_bytes());
    assert_eq!(
        client_record_codec::decode(&buffer).err(),
        Some(Error::Truncated)
    );
}

#[test]
fn client_record_codec_rejects_unsupported_versions_and_trailing_bytes() {
    use client_record_codec::Error;
    let mut buffer = [0; 17];
    client_record_codec::encode(client_source(), 0, &[], &mut buffer).unwrap();
    assert_eq!(
        client_record_codec::decode(&buffer).err(),
        Some(Error::TrailingBytes)
    );
    for version in [0, 2, 255] {
        buffer[0] = version;
        assert_eq!(
            client_record_codec::decode(&buffer[..16]).err(),
            Some(Error::UnsupportedVersion(version))
        );
    }
    let mut binary = [0; 19];
    client_record_codec::encode(client_source(), 0, b"abc", &mut binary).unwrap();
    binary[14..16].copy_from_slice(&2_u16.to_be_bytes());
    assert_eq!(
        client_record_codec::decode(&binary).err(),
        Some(Error::TrailingBytes)
    );
}

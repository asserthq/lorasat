use super::*;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel};
use sat_core::net::{DatagramMetadata, Endpoint};
use sat_drivers::storage::RamStorage;

fn run_ready<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("operation must complete immediately"),
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
        panic!("unexpected datagram receive");
    }
}

#[test]
fn run_sends_one_pong_per_ping_and_continues_waiting() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let mut storage = RamStorage::<32, 2>::new();
    run_ready(storage.append(b"retained")).unwrap();
    let mut payload = PayloadSystem::new(
        MockSocket("beacon"),
        MockSocket(7_u8),
        storage,
        events.dyn_sender(),
        commands.dyn_receiver(),
    );
    commands.try_send(PayloadCommand::Ping).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();

    {
        let mut future = core::pin::pin!(payload.run());
        let mut context = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut context).is_pending());
        for _ in 0..2 {
            assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
        }
        assert!(events.try_receive().is_err());
        assert!(commands.try_receive().is_err());

        commands.try_send(PayloadCommand::Ping).unwrap();
        assert!(future.as_mut().poll(&mut context).is_pending());
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
    let mut payload = PayloadSystem::new(
        MockSocket("beacon"),
        MockSocket(7_u8),
        RamStorage::<32, 2>::new(),
        events.dyn_sender(),
        commands.dyn_receiver(),
    );
    events.try_send(PayloadEvent::Pong).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();

    let mut future = core::pin::pin!(payload.run());
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(commands.len(), 1);
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));

    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(commands.len(), 0);
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));

    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    assert!(events.try_receive().is_err());
}

#[test]
fn cancelled_run_retains_pending_event_for_restart_without_duplicates() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 1>::new();
    let mut payload = PayloadSystem::new(
        MockSocket("beacon"),
        MockSocket(7_u8),
        RamStorage::<32, 2>::new(),
        events.dyn_sender(),
        commands.dyn_receiver(),
    );
    events.try_send(PayloadEvent::Pong).unwrap();
    commands.try_send(PayloadCommand::Ping).unwrap();

    {
        let mut future = core::pin::pin!(payload.run());
        let mut context = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut context).is_pending());
    }
    assert_eq!(payload.pending_event, Some(PayloadEvent::Pong));
    assert!(commands.try_receive().is_err());
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));

    {
        let mut future = core::pin::pin!(payload.run());
        let mut context = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut context).is_pending());
    }
    assert_eq!(events.try_receive(), Ok(PayloadEvent::Pong));
    assert!(events.try_receive().is_err());
    assert_eq!(payload.pending_event, None);
}

#[test]
fn ping_returns_pong_and_consumes_only_one_command() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let mut payload = PayloadSystem::new(
        MockSocket("beacon"),
        MockSocket(7_u8),
        RamStorage::<32, 2>::new(),
        events.dyn_sender(),
        commands.dyn_receiver(),
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
    let mut payload = PayloadSystem::new(
        MockSocket("beacon"),
        MockSocket(7_u8),
        storage,
        events.dyn_sender(),
        commands.dyn_receiver(),
    );

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
    let mut payload = PayloadSystem::new(
        MockSocket("beacon"),
        MockSocket(7_u8),
        RamStorage::<32, 2>::new(),
        events.dyn_sender(),
        commands.dyn_receiver(),
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
fn constructor_stores_separate_protocol_handles() {
    let commands = Channel::<NoopRawMutex, PayloadCommand, 2>::new();
    let events = Channel::<NoopRawMutex, PayloadEvent, 3>::new();
    let payload = PayloadSystem::new(
        MockSocket("beacon"),
        MockSocket(7_u8),
        RamStorage::<32, 2>::new(),
        events.dyn_sender(),
        commands.dyn_receiver(),
    );

    assert_eq!(payload.beacon_socket.0, "beacon");
    assert_eq!(payload.client_socket.0, 7);
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

    let _payload = PayloadSystem::new(
        MockSocket("beacon"),
        MockSocket(7_u8),
        RamStorage::<32, 2>::new(),
        events.dyn_sender(),
        commands.dyn_receiver(),
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
    let payload = PayloadSystem::new(
        MockSocket("beacon"),
        MockSocket(7_u8),
        RamStorage::<32, 2>::new(),
        events.dyn_sender(),
        commands.dyn_receiver(),
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
    let mut payload = PayloadSystem::new(
        MockSocket("beacon"),
        MockSocket(7_u8),
        storage,
        events.dyn_sender(),
        commands.dyn_receiver(),
    );

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

use core::future::pending;

use sat_csp::{
    BindError, Error, Link, MAX_NODE_ID, MAX_PORT, RunnerBuilder, SendError, StackResources,
};

struct NeverLink;

impl Link for NeverLink {
    type Error = ();

    async fn send(&mut self, _frame: &[u8]) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn receive(&mut self, _frame: &mut [u8]) -> Result<usize, Self::Error> {
        pending().await
    }
}

#[test]
fn sockets_bind_to_unique_ports_before_runner_build() {
    let resources = StackResources::<255, 2, 2, 2>::new();
    let mut builder = RunnerBuilder::new(7, NeverLink, &resources).unwrap();

    let command = builder.bind(10).unwrap();
    let telemetry = builder.bind(20).unwrap();

    assert_eq!(command.local_address(), 7);
    assert_eq!(command.local_port(), 10);
    assert_eq!(telemetry.local_port(), 20);
    assert!(matches!(builder.bind(10), Err(BindError::PortInUse(10))));
    assert!(matches!(
        builder.bind(MAX_PORT + 1),
        Err(BindError::InvalidPort(64))
    ));
    assert!(matches!(builder.bind(30), Err(BindError::NoSocketSlots)));

    let _runner = builder.build();
}

#[test]
fn invalid_local_address_is_rejected() {
    let resources = StackResources::<255, 1, 1, 1>::new();
    assert!(matches!(
        RunnerBuilder::new(MAX_NODE_ID + 1, NeverLink, &resources),
        Err(Error::InvalidSource(value)) if value == MAX_NODE_ID + 1
    ));
}

#[test]
fn try_send_reports_full_queue_and_packet_errors() {
    let resources = StackResources::<16, 1, 1, 1>::new();
    let mut builder = RunnerBuilder::new(7, NeverLink, &resources).unwrap();
    let socket = builder.bind(10).unwrap();

    socket.try_send_to(8, 20, b"one").unwrap();
    assert_eq!(socket.try_send_to(8, 20, b"two"), Err(SendError::Busy));
    assert_eq!(
        socket.try_send_to(8, 20, &[0; 11]),
        Err(SendError::Packet(Error::PacketTooLarge {
            maximum: 16,
            actual: 17,
        }))
    );
    assert_eq!(
        socket.try_send_to(MAX_NODE_ID + 1, 20, b"x"),
        Err(SendError::Packet(Error::InvalidDestination(
            MAX_NODE_ID + 1
        )))
    );
}

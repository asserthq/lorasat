use std::time::Duration;

use examples_std::{
    csp_demo::decode_dump,
    mock::{TcpSessionConnector, TcpSessionListener, UdpDatagramSocket},
};
use sat_core::comm::{
    Address, DatagramSocket, Endpoint, ReliableMessageSession, SessionConnector,
    address::BROADCAST_ADDRESS,
};
use sat_drivers::storage::RamStorage;
use sat_subsystems::comm::{CommConfig, CommSystem};

const SATELLITE: Address = Address(601);
const GROUND: Address = Address(602);
const CLIENT: Address = Address(603);
const BEACON_PORT: u8 = 10;
const CLIENT_DATA_PORT: u8 = 11;
const COMMAND_PORT: u8 = 20;

#[tokio::test]
async fn comm_system_works_without_csp_stack() {
    let satellite_client_socket =
        UdpDatagramSocket::bind(Endpoint::new(SATELLITE, CLIENT_DATA_PORT), &[CLIENT])
            .await
            .unwrap();
    let satellite_ground_socket =
        UdpDatagramSocket::bind(Endpoint::new(SATELLITE, BEACON_PORT), &[GROUND])
            .await
            .unwrap();
    let command_listener = TcpSessionListener::bind(Endpoint::new(SATELLITE, COMMAND_PORT))
        .await
        .unwrap();
    let storage = RamStorage::<300, 8>::new();
    let destination = Endpoint::new(BROADCAST_ADDRESS, BEACON_PORT);
    let mut system = CommSystem::new(
        satellite_client_socket,
        satellite_ground_socket,
        command_listener,
        storage,
        CommConfig {
            sat_addr: SATELLITE,
            beacon_interval_sec: 1,
            client_beacon_destination: destination,
            ground_beacon_destination: destination,
        },
    );

    let system_task = tokio::spawn(async move { system.run().await });
    let mut client = UdpDatagramSocket::bind(Endpoint::new(CLIENT, BEACON_PORT), &[])
        .await
        .unwrap();
    let mut ground = UdpDatagramSocket::bind(Endpoint::new(GROUND, BEACON_PORT), &[])
        .await
        .unwrap();

    let mut beacon_buffer = [0_u8; 32];
    tokio::time::timeout(Duration::from_secs(2), client.recv_from(&mut beacon_buffer))
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), ground.recv_from(&mut beacon_buffer))
        .await
        .unwrap()
        .unwrap();

    client
        .send_to(
            Endpoint::new(SATELLITE, CLIENT_DATA_PORT),
            b"opaque subscriber data",
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;

    let mut connector = TcpSessionConnector::new(Endpoint::new(GROUND, 63));
    let mut session = connector
        .connect(Endpoint::new(SATELLITE, COMMAND_PORT))
        .await
        .unwrap();
    session.send(b"dump").await.unwrap();
    let mut response = [0_u8; 244];
    let response_length = session.recv(&mut response).await.unwrap();
    let dump = decode_dump(&response[..response_length]).unwrap();

    assert_eq!(dump.total_records, 1);
    assert_eq!(dump.records.len(), 1);
    assert_eq!(dump.records[0].source, CLIENT.0 as u16);
    assert_eq!(dump.records[0].source_port, BEACON_PORT);
    assert_eq!(dump.records[0].payload, b"opaque subscriber data");

    system_task.abort();
}

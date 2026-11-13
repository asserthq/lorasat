use std::time::Duration as StdDuration;

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, pubsub::PubSubChannel,
};
use embassy_time::Duration;

use sat_core::{
    adcs::{AdcsCommand, AdcsEvent},
    eps::{EpsCommand, EpsEvent},
    net::{Address, DatagramSocket, Endpoint, ReliableSession, SessionConnector},
    protocol::{Beacon, GroundCommand, GroundCommandResponse},
};
use sat_drivers::storage::RamStorage;
use sat_subsystems::cdh::{CdhConfig, CdhSystem, ControllerHandles, SubsystemMasterLink};
use test_utils::host::net::{TcpSessionConnector, TcpSessionListener, UdpDatagramSocket};

const SATELLITE: Address = Address::new_unicast(601).unwrap();
const GROUND: Address = Address::new_unicast(602).unwrap();
const BEACON_PORT: u8 = 10;
const COMMAND_PORT: u8 = 20;

static ADCS_COMMANDS: Channel<CriticalSectionRawMutex, AdcsCommand, 4> = Channel::new();
static ADCS_EVENTS: PubSubChannel<CriticalSectionRawMutex, AdcsEvent, 4, 4, 4> =
    PubSubChannel::new();
static EPS_COMMANDS: Channel<CriticalSectionRawMutex, EpsCommand, 4> = Channel::new();
static EPS_EVENTS: PubSubChannel<CriticalSectionRawMutex, EpsEvent, 4, 4, 4> = PubSubChannel::new();

#[tokio::test]
async fn cdh_works_without_csp_stack() {
    let beacon_socket = UdpDatagramSocket::bind(Endpoint::new(SATELLITE, BEACON_PORT), &[GROUND])
        .await
        .unwrap();
    let ground_listener = TcpSessionListener::bind(Endpoint::new(SATELLITE, COMMAND_PORT))
        .await
        .unwrap();
    let subsystems = ControllerHandles {
        adcs: SubsystemMasterLink {
            cmd_tx: ADCS_COMMANDS.sender(),
            event_rx: ADCS_EVENTS.subscriber().unwrap(),
        },
        eps: SubsystemMasterLink {
            cmd_tx: EPS_COMMANDS.sender(),
            event_rx: EPS_EVENTS.subscriber().unwrap(),
        },
    };
    let mut cdh = CdhSystem::new(
        CdhConfig {
            beacon_interval: Duration::from_millis(20),
            satellite_address: SATELLITE,
            beacon_port: BEACON_PORT,
        },
        subsystems,
        beacon_socket,
        ground_listener,
        RamStorage::<300, 8>::new(),
        RamStorage::<300, 8>::new(),
    );
    let cdh_task = tokio::spawn(async move { cdh.run().await });

    let mut ground_beacons = UdpDatagramSocket::bind(Endpoint::new(GROUND, BEACON_PORT), &[])
        .await
        .unwrap();
    let mut beacon_buffer = [0_u8; 64];
    let metadata = tokio::time::timeout(
        StdDuration::from_secs(1),
        ground_beacons.recv_from(&mut beacon_buffer),
    )
    .await
    .expect("beacon timeout")
    .unwrap();
    let beacon = Beacon::decode(&beacon_buffer[..metadata.length()]).unwrap();
    assert_eq!(beacon.sat_addr, SATELLITE);

    let mut connector = TcpSessionConnector::new(Endpoint::new(GROUND, 63));
    let mut session = connector
        .connect(Endpoint::new(SATELLITE, COMMAND_PORT))
        .await
        .unwrap();
    let mut command_buffer = [0_u8; 16];
    let command = GroundCommand::Ping.encode(&mut command_buffer).unwrap();
    session.send(command).await.unwrap();

    let mut response_buffer = [0_u8; 16];
    let response_length = tokio::time::timeout(
        StdDuration::from_secs(1),
        session.recv(&mut response_buffer),
    )
    .await
    .expect("ground response timeout")
    .unwrap();
    let response = GroundCommandResponse::decode(&response_buffer[..response_length]).unwrap();
    assert_eq!(response, GroundCommandResponse::Received);

    cdh_task.abort();
}

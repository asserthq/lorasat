use examples_std::{
    csp_demo::{
        BEACON_INTERVAL_SECONDS, BEACON_PORT, CLIENT_ADDRESS, CLIENT_DATA_PORT, COMMAND_PORT,
        CspDatagramSocket, CspRdpListener, GROUND_ADDRESS, MTU, SATELLITE_ADDRESS, run_rdp_clock,
    }, mock::{RadioBand, UdpCspInterface, UdpDatagramSocket}, peers::SAT_ADDR,
};
use sat_core::comm::{Address, Endpoint, address::BROADCAST_ADDRESS};
use sat_csp::{RdpConfig, RdpResources, RunnerBuilder, StackResources};
use sat_drivers::storage::RamStorage;
use sat_subsystems::comm::{CommConfig, CommSystem};

const CLIENT_RECORD_MAX_SIZE: usize = 300;
const CLIENT_RECORD_CAPACITY: usize = 16;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::init();

    let peers = [SAT_ADDR, ]

    let client_socket = UdpDatagramSocket::bind(Endpoint::new(Address(0), 0), );

    let mut comm_system = CommSystem::new(
        CspDatagramSocket::new(client_socket),
        CspDatagramSocket::new(ground_socket),
        CspRdpListener::new(command_listener),
        client_storage,
        CommConfig {
            sat_addr: Address(u32::from(SATELLITE_ADDRESS)),
            beacon_interval_sec: BEACON_INTERVAL_SECONDS,
            client_beacon_destination: beacon_destination,
            ground_beacon_destination: beacon_destination,
        },
    );

    println!("satellite: running");
    tokio::select! {
        result = runner_435.run() => panic!("435 Runner stopped: {result:?}"),
        result = runner_868.run() => panic!("868 Runner stopped: {result:?}"),
        result = comm_system.run() => match result {},
        result = run_rdp_clock(&rdp_435) => match result {},
    }
}

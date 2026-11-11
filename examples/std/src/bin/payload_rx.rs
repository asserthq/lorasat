use std::io::{self, Write};

use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel};
use examples_std::{
    csp_demo::{BEACON_PORT, CLIENT_DATA_PORT, CspDatagramSocket, MTU},
    mock::UdpCspInterface,
    payload_demo::{COMMAND_PORT, RX_ADDRESS, RX_NODE, TX_ADDRESS, serve_commands},
};
use sat_core::net::{Address, Endpoint};
use sat_csp::{RunnerBuilder, StackResources};
use sat_drivers::storage::RamStorage;
use sat_subsystems::payload::{
    BeaconConfig, MAX_CLIENT_PAYLOAD_SIZE, MAX_CLIENT_RECORD_SIZE, PayloadCommand, PayloadEvent,
    PayloadSystem,
};
use tokio::time::{Duration, sleep};

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let interface = UdpCspInterface::bind_udp(RX_ADDRESS, &[TX_ADDRESS]).await?;

    let resources = StackResources::<MTU, 2, 3, 2>::new();

    let mut builder = RunnerBuilder::new(RX_NODE, interface, &resources)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let beacon_socket = CspDatagramSocket::new(
        builder
            .bind(BEACON_PORT)
            .map_err(|error| io::Error::other(error.to_string()))?,
    );
    let client_socket = CspDatagramSocket::new(
        builder
            .bind(CLIENT_DATA_PORT)
            .map_err(|error| io::Error::other(error.to_string()))?,
    );
    let mut command_socket = CspDatagramSocket::new(
        builder
            .bind(COMMAND_PORT)
            .map_err(|error| io::Error::other(error.to_string()))?,
    );
    let mut runner = builder.build();

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
    println!("rx: ASP CSP/UDP carrier {RX_ADDRESS} simulates 868 MHz; stop with Ctrl+C");
    println!("rx: ASP broadcast beacons on CSP port {BEACON_PORT}, first after 10 s");
    println!(
        "rx: client data on CSP port {CLIENT_DATA_PORT}: max payload {MAX_CLIENT_PAYLOAD_SIZE} bytes, max record {MAX_CLIENT_RECORD_SIZE} bytes, RAM FIFO depth 16 (no overwrite)"
    );
    println!("rx: confirmations are sent only after PayloadSystem returns Pong");
    io::stdout().flush()?;

    let network = async {
        loop {
            if let Err(error) = runner.run().await {
                eprintln!("rx: CSP runner failed: {error}; restarting");
            }
            sleep(Duration::from_millis(250)).await;
        }
    };

    tokio::select! {
        result = serve_commands(&mut command_socket, commands.dyn_sender(), events.dyn_receiver()) => result,
        () = network => unreachable!("ASP network loop returned"),
        _ = payload.run() => unreachable!("payload loop returned"),
    }
}

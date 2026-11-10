use std::io::{self, Write};

use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel};
use examples_std::{
    csp_demo::{BEACON_PORT, CLIENT_DATA_PORT, CspDatagramSocket, MTU},
    mock::UdpCspInterface,
    payload_demo::{COMMAND_PORT, RX_ADDRESS, RX_NODE, TX_ADDRESS, serve_commands},
};
use sat_csp::{RunnerBuilder, StackResources};
use sat_drivers::storage::RamStorage;
use sat_subsystems::payload::{PayloadCommand, PayloadEvent, PayloadSystem};
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
    let mut payload = PayloadSystem::new(
        beacon_socket,
        client_socket,
        RamStorage::<256, 16>::new(),
        events.dyn_sender(),
        commands.dyn_receiver(),
    );
    println!("rx: listening on CSP over UDP {RX_ADDRESS}; stop with Ctrl+C");
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
        () = network => unreachable!("network loop returned"),
        _ = payload.run() => unreachable!("payload loop returned"),
    }
}

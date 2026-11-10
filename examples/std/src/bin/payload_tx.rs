use std::io::{self, Write};

use examples_std::{
    csp_demo::{CspDatagramSocket, MTU},
    mock::UdpCspInterface,
    payload_demo::{
        COMMAND_PORT, RX_ADDRESS, RX_ENDPOINT, SEND_INTERVAL, TX_ADDRESS, TX_NODE, send_ping,
    },
};
use sat_core::net::DatagramSocket;
use sat_csp::{RunnerBuilder, StackResources};
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let interface = UdpCspInterface::bind_udp(TX_ADDRESS, &[RX_ADDRESS]).await?;
    let resources = StackResources::<MTU, 2, 1, 2>::new();
    let mut builder = RunnerBuilder::new(TX_NODE, interface, &resources)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let socket = CspDatagramSocket::new(
        builder
            .bind(COMMAND_PORT)
            .map_err(|error| io::Error::other(error.to_string()))?,
    );
    let mut runner = builder.build();
    println!("tx: CSP over UDP {TX_ADDRESS} to {RX_ADDRESS}; stop with Ctrl+C");
    io::stdout().flush()?;

    let network = async {
        loop {
            if let Err(error) = runner.run().await {
                eprintln!("tx: CSP runner failed: {error}; restarting");
            }
            sleep(std::time::Duration::from_millis(250)).await;
        }
    };
    tokio::select! {
        result = transmit(socket) => result,
        () = network => unreachable!("network loop returned"),
    }
}

async fn transmit(mut socket: impl DatagramSocket) -> io::Result<()> {
    let mut request_id = 0_u32;
    loop {
        request_id = request_id.wrapping_add(1);
        println!("tx #{request_id}: sending Ping; waiting for execution confirmation");
        io::stdout().flush()?;
        match send_ping(&mut socket, RX_ENDPOINT, request_id).await {
            Ok(()) => println!("tx #{request_id}: execution confirmed by RX"),
            Err(error) => eprintln!("tx #{request_id}: {error}; execution status unknown"),
        }
        io::stdout().flush()?;
        sleep(SEND_INTERVAL).await;
    }
}

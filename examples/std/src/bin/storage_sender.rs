use std::io::{self, Write};

use examples_std::{
    csp_demo::CspDatagramSocket,
    storage_demo::{
        MTU, RECEIVER_ADDRESS, RECEIVER_ENDPOINT, SEND_INTERVAL, SENDER_ADDRESS, SENDER_NODE,
        SENDER_PORT,
    },
};
use sat_core::net::DatagramSocket;
use sat_csp::{RunnerBuilder, StackResources};
use test_utils::host::UdpCspInterface;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let interface = UdpCspInterface::bind_udp(SENDER_ADDRESS, &[RECEIVER_ADDRESS]).await?;
    let resources = StackResources::<MTU, 2, 1, 2>::new();
    let mut builder = RunnerBuilder::new(SENDER_NODE, interface, &resources)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let socket = CspDatagramSocket::new(
        builder
            .bind(SENDER_PORT)
            .map_err(|error| io::Error::other(error.to_string()))?,
    );
    let mut runner = builder.build();
    println!("sender: CSP over UDP {SENDER_ADDRESS} to {RECEIVER_ADDRESS}; stop with Ctrl+C");
    println!("sender: sending once per second; no delivery/storage ACK is used");
    io::stdout().flush()?;

    let network = async {
        loop {
            if let Err(error) = runner.run().await {
                eprintln!("sender: CSP runner failed: {error}; restarting");
            }
            sleep(std::time::Duration::from_millis(250)).await;
        }
    };
    tokio::select! {
        result = send_packets(socket) => result,
        () = network => unreachable!(),
    }
}

async fn send_packets(mut socket: impl DatagramSocket) -> io::Result<()> {
    let mut sequence = 0_u64;
    loop {
        sequence = sequence.wrapping_add(1);
        let payload = format!("packet #{sequence}: temperature=23.5");
        match socket.send_to(RECEIVER_ENDPOINT, payload.as_bytes()).await {
            Ok(()) => println!(
                "queued #{sequence}: {} bytes, text={payload:?}",
                payload.len()
            ),
            Err(error) => eprintln!("sender: CSP enqueue failed: {error:?}"),
        }
        io::stdout().flush()?;
        sleep(SEND_INTERVAL).await;
    }
}

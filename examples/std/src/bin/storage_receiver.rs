use std::io::{self, Write};

use examples_std::{
    csp_demo::CspDatagramSocket,
    mock::UdpCspInterface,
    storage_demo::{
        ENTRY_SIZE, MTU, QUEUE_DEPTH, RECEIVE_BUFFER_SIZE, RECEIVER_ADDRESS, RECEIVER_NODE,
        RECEIVER_PORT, ReceiveStoreError, receive_and_store,
    },
};
use sat_core::{net::DatagramSocket, storage::QueueStorage};
use sat_csp::{RunnerBuilder, StackResources};
use sat_drivers::storage::RamStorage;
use tokio::time::{Duration, sleep};

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let interface = UdpCspInterface::bind_udp(RECEIVER_ADDRESS, &[]).await?;
    let resources = StackResources::<MTU, 2, 1, 2>::new();
    let mut builder = RunnerBuilder::new(RECEIVER_NODE, interface, &resources)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let socket = CspDatagramSocket::new(
        builder
            .bind(RECEIVER_PORT)
            .map_err(|error| io::Error::other(error.to_string()))?,
    );
    let mut runner = builder.build();
    println!("receiver: listening on CSP over UDP {RECEIVER_ADDRESS}; stop with Ctrl+C");
    println!("receiver: CSP node={RECEIVER_NODE}, port={RECEIVER_PORT}, MTU={MTU}");
    println!("receiver: RAM FIFO capacity={QUEUE_DEPTH}, entry limit={ENTRY_SIZE} bytes");
    println!(
        "receiver: full queue rejects new records; stored records are never popped automatically"
    );
    io::stdout().flush()?;

    let network = async {
        loop {
            if let Err(error) = runner.run().await {
                eprintln!("receiver: CSP runner failed: {error}; restarting");
            }
            sleep(Duration::from_millis(250)).await;
        }
    };
    tokio::select! {
        result = store_packets(socket) => result,
        () = network => unreachable!(),
    }
}

async fn store_packets(mut socket: impl DatagramSocket) -> io::Result<()> {
    let mut storage = RamStorage::<ENTRY_SIZE, QUEUE_DEPTH>::new();
    let mut buffer = [0; RECEIVE_BUFFER_SIZE];
    let mut received = 0_u64;
    loop {
        match receive_and_store(&mut socket, &mut storage, &mut buffer).await {
            Ok(metadata) => {
                received = received.saturating_add(1);
                let length = metadata.length();
                println!(
                    "stored #{received}: {length} bytes, source={:?}, queued={}, raw={:02x?}, text={:?}",
                    metadata.source(),
                    storage.len(),
                    &buffer[..length],
                    String::from_utf8_lossy(&buffer[..length]),
                );
            }
            Err(ReceiveStoreError::Storage(error)) => {
                received = received.saturating_add(1);
                eprintln!(
                    "rejected #{received}: storage {:?}: {error:?}; queued={}, earlier entries retained",
                    RamStorage::<ENTRY_SIZE, QUEUE_DEPTH>::error_kind(&error),
                    storage.len(),
                );
            }
            Err(ReceiveStoreError::Transport(error)) => {
                eprintln!("receiver: CSP receive failed: {error:?}");
                sleep(Duration::from_millis(250)).await;
            }
        }
        io::stdout().flush()?;
    }
}

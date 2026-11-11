use examples_std::{
    mock::UdpDatagramSocket,
    peers::{BEACON_PORT, GS_ADDR, SAT_ADDR},
};
use sat_core::{
    net::{DatagramSocket, Endpoint},
    protocol::Beacon,
};

const BEACON_BUFFER_SIZE: usize = 64;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut beacon_socket = UdpDatagramSocket::bind(Endpoint::new(GS_ADDR, BEACON_PORT), &[])
        .await
        .unwrap();
    let mut buffer = [0_u8; BEACON_BUFFER_SIZE];

    println!("ground: waiting for satellite beacons");

    loop {
        let metadata = match beacon_socket.recv_from(&mut buffer).await {
            Ok(metadata) => metadata,
            Err(error) => {
                eprintln!("ground: beacon receive failed: {error:?}");
                continue;
            }
        };

        if metadata.source().addr() != SAT_ADDR {
            println!(
                "ground: ignoring datagram from unexpected endpoint {:?}",
                metadata.source(),
            );
            continue;
        }

        match Beacon::decode(&buffer[..metadata.length()]) {
            Ok(beacon) => println!(
                "ground: beacon from {:?}: timestamp={}ms, interval={}s",
                metadata.source(),
                beacon.timestamp,
                beacon.interval_sec,
            ),
            Err(error) => eprintln!(
                "ground: invalid beacon from {:?}: {error:?}",
                metadata.source(),
            ),
        }
    }
}

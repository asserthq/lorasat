use core::fmt::Debug;

use host_sil::config::SAT_ADDR;
use sat_core::{net::DatagramSocket, protocol::Beacon};

const BEACON_BUFFER_SIZE: usize = 64;

pub async fn receive<S>(mut socket: S) -> !
where
    S: DatagramSocket,
    S::Error: Debug,
{
    let mut buffer = [0; BEACON_BUFFER_SIZE];

    loop {
        let metadata = match socket.recv_from(&mut buffer).await {
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
                "\nbeacon from {:?}: timestamp={}ms, interval={}s",
                metadata.source(),
                beacon.timestamp,
                beacon.interval_sec,
            ),
            Err(error) => eprintln!(
                "\nground: invalid beacon from {:?}: {error:?}",
                metadata.source(),
            ),
        }
    }
}

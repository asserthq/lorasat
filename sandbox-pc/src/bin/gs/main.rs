use log::*;
#[allow(unused_imports)]
use sandbox_pc::mock::{PhyRadioUDP, TransportMockTCP};
use sat_core::comm::address::Address;
use sat_core::comm::{TransportEvent, TransportLayer, TransportSession};
use sat_drivers::proto_impl::link::LinkImpl;
use sat_drivers::proto_impl::transport::TransportImpl;

use std::io;
use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let gs_addr = Address(0x01);
    let sat_addr = Address(0x11);

    let phy = PhyRadioUDP::new(gs_addr, &[sat_addr]).await.unwrap();
    let link = LinkImpl::new(gs_addr, phy);
    let mut transport = TransportImpl::new(link);

    // let mut transport = TransportMockTCP::new(gs_addr, &[]).await?;

    // Канал stdin → async
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            if io::stdin().read_line(&mut line).is_ok() {
                let cmd = line.trim().to_string();
                if !cmd.is_empty() && tx.send(cmd).is_err() {
                    break;
                }
            }
        }
    });

    let mut buf = [0u8; 512];

    info!("Ground station ready ({:?})", gs_addr);
    info!("Commands: ping, fetch, help, quit");

    loop {
        // Сначала проверить буфер команд (неблокирующе)
        while let Ok(cmd) = rx.try_recv() {
            match cmd.as_str() {
                "ping" => {
                    info!("Sending ping to satellite {:?}...", sat_addr);
                    transport
                        .send_datagram(sat_addr, b"ping")
                        .await
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{e:?}")))?;
                }
                "fetch" => {
                    info!("Requesting data from satellite {:?}...", sat_addr);
                    transport
                        .send_datagram(sat_addr, b"fetch")
                        .await
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{e:?}")))?;
                }
                "help" => {
                    info!("Commands: ping, fetch, help, quit");
                }
                "quit" | "exit" => {
                    info!("Shutting down");
                    return Ok(());
                }
                _ => warn!("Unknown command: {}", cmd),
            }
        }

        // Ждать сетевого события (с таймаутом для polling команд)
        match tokio::time::timeout(Duration::from_millis(250), transport.next(&mut buf)).await {
            Ok(Ok(event)) => match event {
                TransportEvent::Beacon { payload } => {
                    if let Ok(beacon) = postcard::from_bytes::<sat_core::entity::Beacon>(payload) {
                        info!(
                            "BEACON | sat={:?} | uptime={}ms | interval={}s",
                            beacon.sat_addr, beacon.timestamp, beacon.interval_sec
                        );
                    } else {
                        info!("BEACON ({} bytes raw)", payload.len());
                    }
                }
                TransportEvent::Datagram { from, data } => {
                    let text = String::from_utf8_lossy(data);
                    info!("DATAGRAM from {:?}: {}", from, text);
                    if data == b"pong" {
                        info!("PONG received!");
                    }
                }
                TransportEvent::Session(s) => {
                    let peer = s.peer_addr();
                    info!("SESSION from {:?}", peer);
                    tokio::spawn(async move {
                        let mut session = s;
                        let mut buf = [0u8; 1024];
                        let mut total = 0usize;
                        loop {
                            match session.recv(&mut buf).await {
                                Ok(data) => {
                                    total += data.len();
                                    info!(
                                        "SESSION data from {:?} ({} bytes, total {}): {}",
                                        peer,
                                        data.len(),
                                        total,
                                        String::from_utf8_lossy(data)
                                    );
                                }
                                Err(e) => {
                                    warn!(
                                        "Session {:?} closed after {} bytes: {:?}",
                                        peer, total, e
                                    );
                                    break;
                                }
                            }
                        }
                    });
                }
            },
            Ok(Err(e)) => {
                error!("Transport error: {:?}", e);
            }
            Err(_elapsed) => {
                // Таймаут — возвращаемся к проверке команд
            }
        }
    }
}

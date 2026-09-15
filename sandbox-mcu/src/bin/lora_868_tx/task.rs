use defmt::{info, warn};
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Ticker};
use heapless::Vec;

use sat_core::layer::app::Message;
use sat_core::layer::link::LinkLayer;
use sat_core::layer::phy::MAX_PHY_PAYLOAD;
use sat_core::layer::transport::{self, Packet, PacketHeader, TransportLayer};
use sat_core::message::{Beacon, Telemetry};
use sat_core::storage::Logger;
use sat_drivers::proto_impl::transport::SimpleTransport;

use super::config::CLIENT_ADDR;

const RECORD_MARKER: u8 = 0x7E;
const RECORD_HEADER_LEN: usize = 1 + 4 + 4;

pub async fn sat_task<L, T>(mut transport: SimpleTransport<L>, beacon: Beacon, mut logger: T)
where
    L: LinkLayer,
    L::Error: defmt::Format,
    T: Logger,
    T::Error: defmt::Format,
{
    info!("satellite online, listening");

    let mut beacon_ticker = Ticker::every(Duration::from_secs(beacon.interval_sec as u64));

    loop {
        match select(recv_packet(&mut transport), beacon_ticker.next()).await {
            Either::First(Some((pkt, src))) => {
                log_received(src, &pkt, &mut logger);
            }
            Either::First(None) => {}
            Either::Second(_) => {
                send_beacon(&mut transport, &beacon).await;
            }
        }
    }
}

async fn recv_packet<L: LinkLayer>(transport: &mut SimpleTransport<L>) -> Option<(Packet, u32)>
where
    L::Error: defmt::Format,
{
    let mut buf = [0u8; MAX_PHY_PAYLOAD];
    match transport.recv_message_with_src(&mut buf).await {
        Ok((pkt, src)) => Some((pkt, src)),
        Err(e) => {
            warn!("rx error: {:?}", e);
            None
        }
    }
}

async fn send_beacon<T: TransportLayer>(transport: &mut T, beacon: &Beacon) {
    let msg = Message::BeaconMsg(beacon.clone());

    let mut buf = [0u8; transport::MAX_TRANSPORT_MESSAGE_PAYLOAD];
    let ser = postcard::to_slice(&msg, &mut buf).unwrap();
    let payload = Vec::<u8, { transport::MAX_TRANSPORT_MESSAGE_PAYLOAD }>::from_slice(ser).unwrap();

    let pkt = Packet {
        header: PacketHeader {
            dest_addr: CLIENT_ADDR,
        },
        payload,
    };

    transport.send_message(pkt).await.unwrap();
    info!("beacon tx -> 0x{:08x}", CLIENT_ADDR);
}

fn log_received<T: Logger>(src_addr: u32, pkt: &Packet, logger: &mut T)
where
    T::Error: defmt::Format,
{
    let when_ms = Instant::now().as_millis() as u32;

    match postcard::from_bytes::<Message>(&pkt.payload) {
        Ok(Message::ClientDataMsg(client_data)) => {
            match postcard::from_bytes::<Telemetry>(&client_data.data) {
                Ok(tm) => {
                    info!(
                        "[rx] from=0x{:08x} when={}ms tm={:?}",
                        src_addr, when_ms, tm
                    );
                    record(logger, src_addr, when_ms, &client_data.data);
                }
                Err(_) => {
                    info!(
                        "[rx] from=0x{:08x} when={}ms data={:?}",
                        src_addr, when_ms, client_data.data
                    );
                }
            }
        }
        Ok(Message::BeaconMsg(_)) => {
            info!(
                "[rx] from=0x{:08x} when={}ms (beacon, ignored)",
                src_addr, when_ms
            );
        }
        Ok(other) => {
            info!(
                "[rx] from=0x{:08x} when={}ms msg={:?}",
                src_addr, when_ms, other
            );
        }
        Err(_) => {
            info!(
                "[rx] from=0x{:08x} when={}ms raw={:?}",
                src_addr, when_ms, pkt.payload
            );
        }
    }
}

fn record<T: Logger>(logger: &mut T, src_addr: u32, when_ms: u32, data: &[u8])
where
    T::Error: defmt::Format,
{
    let mut buf = [0u8; RECORD_HEADER_LEN + 255];

    buf[0] = RECORD_MARKER;
    buf[1..5].copy_from_slice(&src_addr.to_le_bytes());
    buf[5..9].copy_from_slice(&when_ms.to_le_bytes());

    let n = data.len().min(buf.len() - RECORD_HEADER_LEN);
    buf[RECORD_HEADER_LEN..RECORD_HEADER_LEN + n].copy_from_slice(&data[..n]);

    let len = RECORD_HEADER_LEN + n;
    match logger.append(&buf[..len]) {
        Ok(()) => info!("sd: logged {} bytes", len),
        Err(e) => warn!("sd write failed: {:?}", e),
    }
}

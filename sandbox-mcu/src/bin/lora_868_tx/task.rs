use defmt::{info, warn};
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Ticker};

use sat_core::comm::address::{Address, BROADCAST_ADDRESS};
use sat_core::comm::link::LinkLayer;
use sat_core::entity::{Beacon, ClientData};
use sat_core::storage::Logger;

const RECORD_MARKER: u8 = 0x7E;
const RECORD_HEADER_LEN: usize = 1 + 4 + 4;

const RX_BUF_LEN: usize = 255;

pub async fn sat_task<L, T>(mut link: L, beacon: Beacon, mut logger: T)
where
    L: LinkLayer,
    L::Error: defmt::Format,
    T: Logger,
    T::Error: defmt::Format,
{
    info!("satellite online, listening");

    let mut beacon_ticker = Ticker::every(Duration::from_secs(beacon.interval_sec as u64));
    let mut rx_buf = [0u8; RX_BUF_LEN];

    loop {
        match select(recv_frame(&mut link, &mut rx_buf), beacon_ticker.next()).await {
            Either::First(Some((src, payload))) => {
                log_received(src, payload, &mut logger);
            }
            Either::First(None) => {}
            Either::Second(_) => {
                send_beacon(&mut link, &beacon).await;
            }
        }
    }
}

async fn recv_frame<'b, L: LinkLayer>(
    link: &mut L,
    buf: &'b mut [u8],
) -> Option<(Address, &'b [u8])>
where
    L::Error: defmt::Format,
{
    match link.recv_frame(buf).await {
        Ok((src, payload)) => Some((src, payload)),
        Err(e) => {
            warn!("rx error: {:?}", e);
            None
        }
    }
}

async fn send_beacon<L: LinkLayer>(link: &mut L, beacon: &Beacon)
where
    L::Error: defmt::Format,
{
    let mut buf = [0u8; 255];
    match postcard::to_slice(beacon, &mut buf) {
        Ok(ser) => {
            link.send_frame(BROADCAST_ADDRESS, ser).await.unwrap();
            info!("beacon tx -> 0x{:08x}", BROADCAST_ADDRESS.0);
        }
        Err(_) => warn!("beacon serialize failed"),
    }
}

fn log_received<T: Logger>(src_addr: Address, payload: &[u8], logger: &mut T)
where
    T::Error: defmt::Format,
{
    let when_ms = Instant::now().as_millis() as u32;

    match postcard::from_bytes::<ClientData>(payload) {
        Ok(client_data) => {
            info!(
                "[rx] from=0x{:08x} when={}ms data={:?}",
                src_addr.0, when_ms, client_data.data
            );
            record(logger, src_addr.0, when_ms, client_data.data);
        }
        Err(_) => {
            info!(
                "[rx] from=0x{:08x} when={}ms raw={:?}",
                src_addr.0, when_ms, payload
            );
            record(logger, src_addr.0, when_ms, payload);
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

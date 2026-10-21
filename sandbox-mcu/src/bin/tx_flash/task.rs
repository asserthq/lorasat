use defmt::{info, warn};
use embassy_futures::select::{Either, select};
use embassy_stm32::{peripherals, wdg::IndependentWatchdog};
use embassy_time::{Duration, Instant, Ticker};

use sat_core::comm::address::Address;
use sat_core::comm::link::{LinkLayer, RecvFrame};
use sat_core::entity::{Beacon, ClientData, Command, Datagram, Response};
use sat_core::storage::Logger;

use crate::config;
use crate::flash_logger::FlashLogger;

const RECORD_MARKER: u8 = 0x7E;
const RECORD_HEADER_LEN: usize = 1 + 4 + 4;

const RX_BUF_LEN: usize = 255;

pub async fn sat_task<A, G, SPI, DLY>(
    mut asp_link: A,
    mut gs_link: G,
    beacon: Beacon,
    mut logger: FlashLogger<SPI, DLY>,
    mut wdg: IndependentWatchdog<'_, peripherals::IWDG>,
) where
    A: LinkLayer,
    A::Error: defmt::Format,
    G: LinkLayer,
    G::Error: defmt::Format,
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DLY: embedded_hal::delay::DelayNs,
{
    info!("satellite online, listening");

    let mut beacon_ticker = Ticker::every(Duration::from_secs(beacon.interval_sec as u64));
    let mut asp_buf = [0u8; RX_BUF_LEN];
    let mut gs_buf = [0u8; RX_BUF_LEN];

    loop {
        match select(
            recv_frame(&mut asp_link, &mut asp_buf),
            select(recv_frame(&mut gs_link, &mut gs_buf), beacon_ticker.next()),
        )
        .await
        {
            Either::First(Some((src, payload))) => {
                log_received(src, payload, &mut logger);
            }
            Either::First(None) => {}

            Either::Second(Either::First(Some((_src, payload)))) => {
                handle_command(&mut asp_link, &mut gs_link, &mut logger, payload).await;
            }
            Either::Second(Either::First(None)) => {}

            Either::Second(Either::Second(_)) => {
                send_beacon(&mut asp_link, &beacon).await;
            }
        }
        wdg.pet();
    }
}

async fn handle_command<A, G, SPI, DLY>(
    asp_link: &mut A,
    gs_link: &mut G,
    logger: &mut FlashLogger<SPI, DLY>,
    payload: &[u8],
) where
    A: LinkLayer,
    A::Error: defmt::Format,
    G: LinkLayer,
    G::Error: defmt::Format,
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DLY: embedded_hal::delay::DelayNs,
{
    let cmd = match postcard::from_bytes::<Datagram>(payload) {
        Ok(Datagram::Command(cmd)) => cmd,
        Ok(_) => {
            warn!("gs: got non-command datagram");
            return;
        }
        Err(_) => {
            warn!("gs: command decode failed");
            return;
        }
    };

    match cmd {
        Command::RequestClientData => {
            info!("gs: command RequestClientData, draining flash");
            drain(asp_link, gs_link, logger).await;
            send_response(gs_link, Response::Executed).await;
        }
        other => {
            warn!("gs: unhandled command {:?}", other);
            send_response(gs_link, Response::Failed).await;
        }
    }
}

async fn drain<A, G, SPI, DLY>(
    asp_link: &mut A,
    gs_link: &mut G,
    logger: &mut FlashLogger<SPI, DLY>,
) where
    A: LinkLayer,
    A::Error: defmt::Format,
    G: LinkLayer,
    G::Error: defmt::Format,
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DLY: embedded_hal::delay::DelayNs,
{
    let mut asp_buf = [0u8; RX_BUF_LEN];

    loop {
        match select(
            recv_frame(asp_link, &mut asp_buf),
            downlink_step(gs_link, logger),
        )
        .await
        {
            Either::First(Some((src, payload))) => {
                log_received(src, payload, logger);
            }
            Either::First(None) => {}
            Either::Second(true) => {}
            Either::Second(false) => {
                info!("gs: drain complete");
                break;
            }
        }
    }
}

async fn downlink_step<G, SPI, DLY>(gs_link: &mut G, logger: &mut FlashLogger<SPI, DLY>) -> bool
where
    G: LinkLayer,
    G::Error: defmt::Format,
    SPI: embedded_hal::spi::SpiDevice<u8>,
    DLY: embedded_hal::delay::DelayNs,
{
    let mut buf = [0u8; 4096];

    let rec = match logger.read_next(&mut buf) {
        Ok(Some(rec)) => rec,
        Ok(None) => return false,
        Err(e) => {
            warn!("downlink: read failed: {:?}", e);
            return false;
        }
    };

    if let Err(e) = gs_link.send_frame(Address(config::GS_ADDR), rec.data).await {
        warn!("downlink: send failed: {:?}", e);
        return false;
    }

    info!("downlink: sent seq={} len={}", rec.seq, rec.data.len());
    true
}

async fn send_response<G: LinkLayer>(gs_link: &mut G, resp: Response)
where
    G::Error: defmt::Format,
{
    let mut buf = [0u8; 255];
    let datagram = Datagram::Response(resp);
    match postcard::to_slice(&datagram, &mut buf) {
        Ok(ser) => {
            gs_link
                .send_frame(Address(config::GS_ADDR), ser)
                .await
                .unwrap();
            info!("gs: response {:?}", resp);
        }
        Err(_) => warn!("gs: response serialize failed"),
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
        Ok(RecvFrame::Data { src, payload }) => Some((src, payload)),
        Ok(RecvFrame::Beacon { .. }) => None,
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
            link.send_beacon(ser).await.unwrap();
            info!("beacon tx (sat=0x{:08x})", beacon.sat_addr.0);
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
        Ok(()) => info!("flash: logged {} bytes", len),
        Err(e) => warn!("flash write failed: {:?}", e),
    }
}

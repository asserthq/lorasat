use std::{future::pending, time::Duration};

use host_sil::config::{
    BEACON_INTERVAL_SECS, BEACON_PORT, CLIENT_ADDR, CLIENT_DATA_PORT, MTU, SAT_ADDR,
};
use sat_core::{
    net::{DatagramSocket, Endpoint},
    protocol::Beacon,
};
use sat_csp::{HEADER_SIZE, Socket};
use tokio::time::{Instant, sleep, sleep_until, timeout_at};

use crate::{
    aloha::{self, Slot},
    generator,
    random::Random,
};

const MAX_APPLICATION_PAYLOAD: usize = MTU - HEADER_SIZE;
const RECEIVE_ERROR_BACKOFF: Duration = Duration::from_millis(100);

struct PendingTransmission {
    beacon: Beacon,
    sample_id: u32,
    slot: Slot,
}

pub async fn run<B, const TX_DEPTH: usize, const RX_DEPTH: usize>(
    mut beacon_socket: B,
    data_socket: Socket<'_, MTU, TX_DEPTH, RX_DEPTH>,
    mut random: Random,
) -> !
where
    B: DatagramSocket,
{
    let beacon_source = Endpoint::new(SAT_ADDR, BEACON_PORT);
    let mut beacon_buffer = [0_u8; MAX_APPLICATION_PAYLOAD];
    let mut data_buffer = [0_u8; MAX_APPLICATION_PAYLOAD];
    let mut beacon_count = 0_u32;
    let mut last_timestamp = None;
    let mut scheduled: Option<PendingTransmission> = None;
    let mut wait_timeout = Duration::from_secs(2 * u64::from(BEACON_INTERVAL_SECS.max(1)));
    let mut wait_deadline = Instant::now() + wait_timeout;

    println!(
        "asp: slotted ALOHA, slot={}ms, beacon guard={}ms; waiting for {beacon_source:?} on 868 MHz",
        aloha::SLOT_MS,
        aloha::BEACON_GUARD_MS,
    );

    loop {
        if Instant::now() >= wait_deadline {
            println!("asp: beacon timeout; discarding pending slot and resynchronizing");
            scheduled = None;
            last_timestamp = None;
            wait_deadline = Instant::now() + wait_timeout;
        }

        let slot_deadline = scheduled
            .as_ref()
            .map(|transmission| transmission.slot.opens_at);
        let received = tokio::select! {
            biased;
            result = timeout_at(wait_deadline, beacon_socket.recv_from(&mut beacon_buffer)) => result,
            () = wait_for_slot(slot_deadline) => {
                let transmission = scheduled.take().expect("slot timer requires a pending transmission");
                if Instant::now() >= transmission.slot.closes_at {
                    eprintln!("asp: sample={} missed its ALOHA slot; waiting for another beacon", transmission.sample_id);
                    continue;
                }

                let telemetry = generator::generate(
                    CLIENT_ADDR,
                    transmission.sample_id,
                    transmission.beacon.timestamp,
                    &mut random,
                );
                let payload = match postcard::to_slice(&telemetry, &mut data_buffer) {
                    Ok(payload) => payload,
                    Err(error) => {
                        eprintln!("asp: telemetry encode failed: {error}");
                        continue;
                    }
                };
                if Instant::now() >= transmission.slot.closes_at {
                    eprintln!("asp: sample={} expired during preparation; skipping", transmission.sample_id);
                    continue;
                }

                match data_socket.try_send_to(SAT_ADDR.get(), CLIENT_DATA_PORT, payload) {
                    Ok(()) => println!(
                        "asp: slot={}/{}, {} telemetry bytes queued to local CSP transport: {telemetry:?}; receipt and storage are not confirmed",
                        transmission.slot.index,
                        transmission.slot.count,
                        payload.len(),
                    ),
                    Err(error) => eprintln!(
                        "asp: sample={} could not enter the local TX queue: {error:?}; waiting for another beacon",
                        transmission.sample_id,
                    ),
                }
                continue;
            }
        };

        let metadata = match received {
            Ok(Ok(metadata)) => metadata,
            Ok(Err(error)) => {
                eprintln!("asp: beacon receive failed: {error:?}");
                if let Some(metadata) = beacon_socket.discard_oversized(beacon_buffer.len()) {
                    eprintln!("asp: discarded oversized beacon datagram: {metadata:?}");
                }
                sleep(RECEIVE_ERROR_BACKOFF).await;
                continue;
            }
            Err(_) => {
                scheduled = None;
                last_timestamp = None;
                wait_deadline = Instant::now() + wait_timeout;
                println!("asp: still waiting for a valid beacon; no telemetry sent");
                continue;
            }
        };
        let received_at = Instant::now();

        if metadata.source() != beacon_source {
            eprintln!(
                "asp: ignoring datagram from unexpected endpoint {:?}",
                metadata.source()
            );
            continue;
        }
        let Some(payload) = beacon_buffer.get(..metadata.length()) else {
            eprintln!("asp: ignoring beacon with invalid datagram length: {metadata:?}");
            continue;
        };
        let beacon = match Beacon::decode(payload) {
            Ok(beacon) => beacon,
            Err(error) => {
                eprintln!("asp: ignoring invalid native beacon: {error:?}");
                continue;
            }
        };
        if beacon.sat_addr != SAT_ADDR || beacon.interval_sec == 0 {
            eprintln!(
                "asp: ignoring beacon with unexpected satellite or zero interval: {beacon:?}"
            );
            continue;
        }
        if last_timestamp.is_some_and(|timestamp| beacon.timestamp <= timestamp) {
            println!(
                "asp: ignoring duplicate or older beacon timestamp={}",
                beacon.timestamp
            );
            continue;
        }

        last_timestamp = Some(beacon.timestamp);
        beacon_count = beacon_count.wrapping_add(1);
        wait_timeout = Duration::from_secs(2 * u64::from(beacon.interval_sec));
        wait_deadline = received_at + wait_timeout;

        if scheduled.is_some() {
            println!("asp: new beacon replaces the pending ALOHA slot");
        }
        scheduled = aloha::choose_slot(received_at, beacon.interval_sec, &mut random).map(|slot| {
            PendingTransmission {
                beacon,
                sample_id: beacon_count,
                slot,
            }
        });

        match &scheduled {
            Some(transmission) => println!(
                "asp: beacon #{beacon_count}, timestamp={}ms, interval={}s; slot={}/{}, delay={}ms",
                beacon.timestamp,
                beacon.interval_sec,
                transmission.slot.index,
                transmission.slot.count,
                u64::from(transmission.slot.index) * aloha::SLOT_MS,
            ),
            None => println!(
                "asp: beacon #{beacon_count} leaves no complete ALOHA slot after the guard; skipping TX"
            ),
        }
    }
}

async fn wait_for_slot(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => pending::<()>().await,
    }
}

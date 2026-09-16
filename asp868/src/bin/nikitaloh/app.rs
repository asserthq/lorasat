use core::sync::atomic::Ordering;

use defmt::info;
use embassy_time::Timer;

use sat_core::layer::transport::TransportLayer;
use sat_core::message::Telemetry;

use super::identity;
use super::led::BLINK_FLAG;
use super::link;

const SLOT_MS: u32 = 100;
const BEACON_GUARD_MS: u32 = 500;

pub async fn run<T: TransportLayer>(mut transport: T, node_addr: u32) {
    let mut beacon_count: u32 = 0;

    loop {
        info!("Waiting for beacon...");

        let beacon = link::wait_beacon(&mut transport).await;

        beacon_count += 1;

        info!(
            "Beacon #{}: sat=0x{:08x} interval={}s ts={}",
            beacon_count, beacon.sat_addr, beacon.interval_sec, beacon.timestamp
        );

        let interval_ms = (beacon.interval_sec as u32).saturating_mul(1000);
        let usable_ms = interval_ms.saturating_sub(BEACON_GUARD_MS);
        let num_slots = usable_ms / SLOT_MS;

        let delay_ms = if num_slots > 0 {
            let slot = identity::rand_u32() % num_slots;
            slot * SLOT_MS
        } else {
            0
        };

        info!(
            "ALOHA slot = {}/{} delay = {} ms",
            delay_ms / SLOT_MS,
            num_slots,
            delay_ms
        );

        Timer::after_millis(delay_ms as u64).await;

        BLINK_FLAG.store(true, Ordering::Relaxed);
        info!("TX slot fired at delay={}ms", delay_ms);

        let tm = Telemetry {
            node_addr,
            sample_id: beacon_count,
            beacon_timestamp: beacon.timestamp,
            temp: 20.0 + (identity::rand_u32() % 150) as f32 / 10.0,
            bat_voltage: 3.3 + (identity::rand_u32() % 7) as f32 / 10.0,
        };

        link::send_client_telemetry(&mut transport, beacon.sat_addr, &tm).await;

        Timer::after_millis(1500).await;
    }
}

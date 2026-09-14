use core::sync::atomic::Ordering;

use defmt::info;
use embassy_time::Timer;

use sat_core::layer::transport::TransportLayer;
use sat_core::message::Telemetry;

use super::identity;
use super::led::BLINK_FLAG;
use super::link;

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

        let max_delay_ms = (beacon.interval_sec as u32).saturating_mul(1000);
        let delay_ms = if max_delay_ms > 0 {
            let r = identity::rand_u32()
                .wrapping_add(beacon.timestamp.wrapping_mul(0x9E37_79B9))
                .wrapping_add(beacon_count.wrapping_mul(0x85EB_CA6B));
            r % max_delay_ms
        } else {
            0
        };

        info!("ALOHA delay = {} ms", delay_ms);

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

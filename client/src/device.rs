use core::sync::atomic::Ordering;

use defmt::*;
use embassy_time::Timer;
use sat_core::{
    comm::{Address, LinkLayer},
    entity::{Beacon, ClientData},
};

use crate::{identity, led::BLINK_FLAG, tm::Telemetry};

const SLOT_MS: u32 = 100;
const BEACON_GUARD_MS: u32 = 500;

pub struct Device<L: LinkLayer> {
    link: L,
}

impl<L: LinkLayer> Device<L> {
    pub fn new(link: L) -> Self {
        Self { link }
    }

    pub async fn run(&mut self) {
        let mut beacon_count: u32 = 0;

        loop {
            info!("Waiting for beacon...");

            let beacon = self.wait_beacon().await;

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
                node_addr: self.link.addr().0,
                sample_id: beacon_count,
                beacon_timestamp: beacon.timestamp,
                temp: 20.0 + (identity::rand_u32() % 150) as f32 / 10.0,
                bat_voltage: 3.3 + (identity::rand_u32() % 7) as f32 / 10.0,
            };

            self.send_client_telemetry(beacon.sat_addr.0, &tm).await;

            Timer::after_millis(1500).await;
        }
    }

    pub async fn wait_beacon(&mut self) -> Beacon {
        let mut buf = [0u8; 255];

        loop {
            let (_, frame) = match self.link.recv_frame(&mut buf).await {
                Ok(x) => x,
                Err(_) => {
                    Timer::after_millis(100).await;
                    continue;
                }
            };

            match postcard::from_bytes::<Beacon>(&frame) {
                Ok(beacon) => return beacon,
                Err(_) => info!("not beacon or decode failed"),
            }
        }
    }

    pub async fn send_client_telemetry(&mut self, dest: u32, tm: &Telemetry) {
        let mut data_buf = [0u8; 255];
        let tm_ser = postcard::to_slice(tm, &mut data_buf).unwrap();

        let msg = ClientData { data: tm_ser };

        let mut buf = [0u8; 255];
        let payload = postcard::to_slice(&msg, &mut buf).unwrap();

        info!("tx ClientData -> 0x{:08x}: {:?}", dest, tm);
        self.link.send_frame(Address(dest), payload).await.unwrap();
        info!("tx done");
    }
}

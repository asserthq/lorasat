use std::time::{Duration, SystemTime, UNIX_EPOCH};

use heapless::Vec;
use sat_core::layer::app::AppMessage;
use sat_core::layer::transport::{TransportLayer, TransportMessage};
use sat_core::message::beacon::Beacon;
use sat_core::message::client_data::ClientData;

use crate::telemetry::{self, Telemetry, TelemetryVec};

pub struct ClientDevice<T: TransportLayer> {
    transport: T,
    pending_telemetry: TelemetryVec,
    sample_count: u64,
    idle_ticks: u64,
    rng_state: u64,
}

impl<T: TransportLayer> ClientDevice<T> {
    pub fn new(transport: T) -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        Self {
            transport,
            pending_telemetry: Vec::new(),
            sample_count: 0,
            idle_ticks: 0,
            rng_state: if seed == 0 { 1 } else { seed },
        }
    }

    pub async fn run(mut self) {
        let first_tlm = Duration::from_secs(self.rand_range(3, 8));
        let mut tlm_timer = tokio::time::interval(first_tlm);
        let mut idle = tokio::time::interval(Duration::from_millis(100));

        println!("[client] online, listening");

        loop {
            tokio::select! {
                beacon = self.wait_beacon() => {
                    println!("[client] rx beacon: {beacon:?}");
                    self.flush_telemetry(beacon.sat_addr).await
                }

                _ = tlm_timer.tick() => {
                    self.collect_telemetry();
                    let next = Duration::from_secs(self.rand_range(3, 8));
                    tlm_timer.reset_after(next);
                }

                _ = idle.tick() => {
                    self.idle_ticks += 1;
                }
            }
        }
    }

    async fn wait_beacon(&mut self) -> Beacon {
        loop {
            let mut buf = [0u8; 4096];
            let msg = self.transport.try_recv_message(&mut buf).await.unwrap();
            if let Ok(AppMessage::BeaconMsg(beacon)) = postcard::from_bytes(&msg.payload) {
                return beacon;
            }
        }
    }

    /// Sends accumulated telemetry to the satellite.
    async fn flush_telemetry(&mut self, sat_addr: u32) {
        if self.pending_telemetry.is_empty() {
            return;
        }

        // Serialize telemetry vec into bytes, then wrap in ClientData.
        let mut tm_buf = [0u8; 256];
        let tm_slice = telemetry::try_encode_vec(&self.pending_telemetry, &mut tm_buf)
            .expect("tm encode error");
        let data = Vec::<u8, 256>::from_slice(tm_slice).expect("tm fits in 256 bytes");

        let msg = AppMessage::ClientDataMsg(ClientData { data });

        let mut buf = [0u8; 4096];
        let ser = postcard::to_slice(&msg, &mut buf).expect("serialize AppMessage");
        let payload = Vec::<u8, 4096>::from_slice(ser).expect("payload fits");

        // dest_addr = satellite address, flows through the message.
        let transport_msg = TransportMessage {
            dest_addr: sat_addr,
            payload,
        };

        println!(
            "[client] tx telemetry ({} samples)",
            self.pending_telemetry.len()
        );
        self.transport
            .try_send_message(transport_msg)
            .await
            .unwrap();
        self.pending_telemetry.clear();
    }

    fn collect_telemetry(&mut self) {
        let tm = Telemetry {
            temp: 20.0 + self.rand_float() * 15.0,
            bat_voltage: 3.3 + self.rand_float() * 0.7,
            rssi: -((40 + self.rand_u64() % 51) as i32),
        };

        self.pending_telemetry
            .push(tm.clone())
            .expect("tm push error");

        self.sample_count += 1;
        println!("[client] collect tm #{}: {tm:?}", self.sample_count);
    }

    // --- Minimal LCG RNG ---

    fn lcg_step(&mut self) -> u64 {
        self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345);
        self.rng_state
    }

    fn rand_u64(&mut self) -> u64 {
        self.lcg_step()
    }

    fn rand_float(&mut self) -> f32 {
        (self.lcg_step() & 0xFFFF) as f32 / 65536.0
    }

    fn rand_range(&mut self, min: u64, max: u64) -> u64 {
        min + (self.lcg_step() % (max - min))
    }
}

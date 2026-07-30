use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sat_core::error::Error;
use sat_core::protocol::{Beacon, DataFrame, Frame};
use sat_core::radio::HalfDuplexTransceiver;

const SAT_ADDR: u32 = 9001;

pub struct Satellite<R: HalfDuplexTransceiver> {
    radio: R,
    buf: [u8; 256],
    idle_ticks: u64,
    beacon_interval_sec: u16,
}

impl<R: HalfDuplexTransceiver> Satellite<R> {
    pub fn new(radio: R) -> Self {
        Self {
            radio,
            buf: [0u8; 256],
            idle_ticks: 0,
            beacon_interval_sec: 10,
        }
    }

    /// Главный цикл — приём + маяк каждые 10 сек + idle каждые 100 мс.
    pub async fn run(mut self) {
        let mut beacon = tokio::time::interval(Duration::from_secs(10));
        let mut idle = tokio::time::interval(Duration::from_millis(100));

        println!("[sat] online, listening");

        loop {
            tokio::select! {
                res = self.receive_tm() => {
                    match res {
                        Ok(data_frame) => {
                            println!("[sat] rx data: {data_frame:?}");
                        }
                        Err(e) => {
                            eprintln!("[sat] rx error: {e:?}");
                        }
                    }
                }

                _ = beacon.tick() => {
                    self.send_beacon().await;
                }

                _ = idle.tick() => {
                    self.idle_ticks += 1;
                }
            }
        }
    }

    async fn send_beacon(&mut self) {
        let beacon = self.create_beacon();
        println!("[sat] tx beacon: {beacon:?}");
        let frame = Frame::Beacon(beacon);
        let payload_vec = frame.encode();
        let payload = &payload_vec.as_slice();
        match self.radio.transmit(payload).await {
            Err(e) => {
                eprintln!("[sat radio] tx error: {e:?}");
            }
            Ok(n) => {
                println!("[sat radio] tx ok: {n} bytes");
            }
        }
    }

    async fn receive_tm(&mut self) -> Result<DataFrame, Error> {
        match self.radio.receive(&mut self.buf).await {
            Ok(_) => {
                let frame = Frame::try_decode(&self.buf).unwrap();
                match frame {
                    Frame::Data(data_frame) => Ok(data_frame),
                    _ => Err(Error("rx not data")),
                }
            }
            Err(_) => Err(Error("radio rx error")),
        }
    }

    fn create_beacon(&self) -> Beacon {
        let unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        Beacon {
            sat_addr: SAT_ADDR,
            interval_sec: self.beacon_interval_sec,
            timestamp: unix_secs,
        }
    }
}

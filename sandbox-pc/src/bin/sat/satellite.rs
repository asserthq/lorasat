use std::time::{Duration, SystemTime, UNIX_EPOCH};

use heapless::Vec;
use sat_core::message::{Beacon, Command, Data, DataKind, Frame};
use sat_core::radio::HalfDuplexTransceiver;

const SAT_ADDR: u32 = 1001;
const GS_ADDR: u32 = 1;

pub struct Satellite<R: HalfDuplexTransceiver> {
    radio435: R,
    radio868: R,
    idle_ticks: u64,
    beacon_interval_sec: u16,
}

impl<R: HalfDuplexTransceiver> Satellite<R> {
    pub fn new(radio435: R, radio868: R) -> Self {
        Self {
            radio435,
            radio868,
            idle_ticks: 0,
            beacon_interval_sec: 10,
        }
    }

    /// Главный цикл — приём + маяк каждые 10 сек + idle каждые 100 мс.
    pub async fn run(mut self) {
        let mut beacon = tokio::time::interval(Duration::from_secs(10));
        let mut idle = tokio::time::interval(Duration::from_millis(100));

        let mut buf435 = [0u8; 256];
        let mut buf868 = [0u8; 256];

        println!("[sat] online, listening");

        loop {
            let rx435 = Self::receive_cmd_435(&mut self.radio435, &mut buf435);
            let rx868 = Self::receive_tm_868(&mut self.radio868, &mut buf868);

            tokio::select! {
                data_frame = rx868 => {
                    println!("[sat] rx [868] data: {data_frame:?}");
                }

                cmd = rx435 => {
                    match cmd {
                        Some(cmd) => {
                            println!("[sat] rx [435] cmd: {cmd:?}");
                            self.handle_command(cmd).await;
                        }
                        None => {
                            eprintln!("[sat] rx [435] no cmd");
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
        let frame = Frame::BeaconFrame(beacon);
        let mut buf = [0u8; 256];
        let payload = frame
            .try_encode(&mut buf)
            .expect("beacon frame encode error");
        match self.radio868.transmit(payload).await {
            Err(e) => {
                eprintln!("[sat radio 868] tx error: {e:?}");
            }
            Ok(n) => {
                println!("[sat radio 868] tx ok: {n} bytes");
            }
        }
        match self.radio435.transmit(payload).await {
            Err(e) => {
                eprintln!("[sat radio 435] tx error: {e:?}");
            }
            Ok(n) => {
                println!("[sat radio 435] tx ok: {n} bytes");
            }
        }
    }

    async fn receive_tm_868(radio868: &mut R, buf: &mut [u8]) -> Option<Data> {
        let n = radio868.receive(buf).await.expect("radio 868 rx error");
        println!("[sat radio 868] rx ok {n} bytes");
        let frame = Frame::try_decode(&buf).expect("frame decode error");
        match frame {
            Frame::DataFrame(data) => Some(data),
            _ => None,
        }
    }

    async fn receive_cmd_435(radio435: &mut R, buf: &mut [u8]) -> Option<Command> {
        let n = radio435
            .receive(buf)
            .await
            .expect("radio [435] receive error");
        println!("[sat radio 435] rx ok {n} bytes");
        let frame = Frame::try_decode(&buf).expect("frame decode error");

        match frame {
            Frame::DataFrame(data) => {
                if data.kind == DataKind::GroundCommand {
                    let cmd = Command::try_decode(&data.data).expect("command decode error");
                    Some(cmd)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    async fn handle_command(&mut self, cmd: Command) {
        match cmd {
            Command::RequestTelemetry => {
                self.send_telemetry().await;
            }
            _ => {}
        }
    }

    async fn send_telemetry(&mut self) {
        let frame = Frame::DataFrame(Data {
            kind: DataKind::SatelliteData,
            src_addr: SAT_ADDR,
            dest_addr: GS_ADDR,
            flags: Default::default(),
            data: Vec::from_array([0, 1, 2, 3, 4, 5, 6, 7]),
        });
        let mut buf = [0u8; 256];
        let payload = frame.try_encode(&mut buf).expect("data frame encode error");
        println!("[gs] tx data: {frame:?}");

        let n = self
            .radio435
            .transmit(payload)
            .await
            .expect("radio tx error");
        println!("[gs radio] tx ok {n} bytes");
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

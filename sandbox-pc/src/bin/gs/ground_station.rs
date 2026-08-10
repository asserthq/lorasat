use core::time::Duration;
use sat_core::message::{Beacon, Command, Data, DataKind, Frame};

use heapless::Vec;
use sat_core::radio::HalfDuplexTransceiver;
use tokio::select;

const SAT_ADDR: u32 = 1001;
const GS_ADDR: u32 = 1;

pub struct GroundStation<R: HalfDuplexTransceiver> {
    radio435: R,
}

impl<R: HalfDuplexTransceiver> GroundStation<R> {
    pub fn new(radio435: R) -> Self {
        Self { radio435 }
    }

    pub async fn run(&mut self) {
        let mut tm_request = tokio::time::interval(Duration::from_secs(10));

        loop {
            select! {
                beacon = self.wait_beacon() => {
                    match beacon {
                        Some(beacon) => println!("[client] rx beacon: {:?}", beacon),
                        None => println!("[client] rx no beacon"),
                    }
                }
                _ = tm_request.tick() => {
                    self.send_command().await;
                }
            }
        }
    }

    async fn send_command(&mut self) {
        let cmd = Command::RequestTelemetry;
        let mut buf = [0u8; 256];
        let filled = cmd.try_encode(&mut buf).expect("command encode error");

        let data = Vec::<u8, 256>::from_slice(filled).expect("cmd fits in 256 bytes");

        let frame = Frame::DataFrame(Data {
            kind: DataKind::GroundCommand,
            src_addr: GS_ADDR,
            dest_addr: SAT_ADDR,
            flags: Default::default(),
            data,
        });

        let payload = frame.try_encode(&mut buf).expect("encode command error");
        self.radio435
            .transmit(&payload)
            .await
            .expect("tx command error");
    }

    async fn wait_beacon(&mut self) -> Option<Beacon> {
        let mut buf = [0u8; 256];
        let n = self
            .radio435
            .receive(&mut buf)
            .await
            .expect("radio [435] rx error");
        println!("[client radio] rx ok {n} bytes");
        let frame = Frame::try_decode(&buf).expect("decode frame error");
        match frame {
            Frame::BeaconFrame(beacon) => Some(beacon),
            _ => None,
        }
    }
}

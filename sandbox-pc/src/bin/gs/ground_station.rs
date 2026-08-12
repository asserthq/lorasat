use core::time::Duration;
use sandbox_pc::addr::SAT_ADDR;
use sat_core::layer::app::AppMessage;
use sat_core::layer::transport::{TransportLayer, TransportMessage};
use sat_core::message::{Beacon, GroundCommand};

use heapless::Vec;
use tokio::select;

pub struct GroundStation<T: TransportLayer> {
    transport: T,
}

impl<T: TransportLayer> GroundStation<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub async fn run(&mut self) {
        let mut tm_request = tokio::time::interval(Duration::from_secs(10));

        loop {
            select! {
                beacon = self.wait_beacon() => {
                    match beacon {
                        Some(beacon) => println!("[gs] rx beacon: {:?}", beacon),
                        None => println!("[gs] rx no beacon"),
                    }
                }
                _ = tm_request.tick() => {
                    self.send_command().await;
                }
            }
        }
    }

    async fn send_command(&mut self) {
        let cmd = GroundCommand::RequestTelemetry;
        let msg = AppMessage::GndCommandMsg(cmd);

        let mut buf = [0u8; 4096];
        let ser = postcard::to_slice(&msg, &mut buf).expect("serialize AppMessage");
        let payload = Vec::<u8, 4096>::from_slice(ser).expect("cmd fits in transport payload");

        // dest_addr flows through the message
        let transport_msg = TransportMessage {
            payload,
            dest_addr: SAT_ADDR,
        };

        self.transport
            .try_send_message(transport_msg)
            .await
            .unwrap();
        println!("[gs] tx RequestTelemetry");
    }

    async fn wait_beacon(&mut self) -> Option<Beacon> {
        let mut buf = [0u8; 4096];
        let transport_msg = self.transport.try_recv_message(&mut buf).await.unwrap();

        let app_msg: AppMessage = postcard::from_bytes(&transport_msg.payload).unwrap();

        match app_msg {
            AppMessage::BeaconMsg(beacon) => Some(beacon),
            _ => None,
        }
    }
}

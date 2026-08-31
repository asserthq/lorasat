use std::time::{Duration, SystemTime, UNIX_EPOCH};

use heapless::Vec;
use sandbox_pc::addr::{CLIENT_ADDR, GS_ADDR, SAT_ADDR};
use sat_core::layer::app::AppMessage;
use sat_core::layer::transport::{
    MAX_TRANSPORT_CHUNK_PAYLOAD, MAX_TRANSPORT_MESSAGE_PAYLOAD, TransportHeader, TransportLayer,
    TransportMessage,
};
use sat_core::message::{Beacon, ClientData, GroundCommand, SatelliteData};

pub struct Satellite<T: TransportLayer> {
    transport_gs: T,
    transport_client: T,
    idle_ticks: u64,
    beacon_interval_sec: u16,
}

impl<T: TransportLayer> Satellite<T> {
    pub fn new(transport_gs: T, transport_client: T) -> Self {
        Self {
            transport_gs,
            transport_client,
            idle_ticks: 0,
            beacon_interval_sec: 10,
        }
    }

    pub async fn run(mut self) {
        let mut beacon = tokio::time::interval(Duration::from_secs(10));
        let mut idle = tokio::time::interval(Duration::from_millis(100));

        println!("[sat] online, listening");

        loop {
            let rx_gs = Self::recv_msg(&mut self.transport_gs);
            let rx_client = Self::recv_msg(&mut self.transport_client);

            tokio::select! {
                client_msg = rx_client => {
                    print!("[sat] [rx_client] ");
                    match client_msg {
                        AppMessage::ClientDataMsg(client_data) => println!("{:?}", client_data),
                        msg => println!("nonsense: {:?}", msg)
                    };
                }

                gs_msg = rx_gs => {
                    print!("[sat] [rx_client] ");
                    match gs_msg {
                        AppMessage::GndCommandMsg(gnd_cmd) => self.handle_command(gnd_cmd).await,
                        msg => println!("nonsense: {:?}", msg),
                    };
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

        let mut buf = [0u8; MAX_TRANSPORT_CHUNK_PAYLOAD];
        let msg = AppMessage::BeaconMsg(beacon);
        let ser = postcard::to_slice(&msg, &mut buf).unwrap();
        let payload = Vec::<u8, MAX_TRANSPORT_MESSAGE_PAYLOAD>::from_slice(ser).unwrap();

        // Send to client (868 MHz).
        let client_msg = TransportMessage {
            header: TransportHeader {
                dest_addr: CLIENT_ADDR,
            },
            payload: payload.clone(),
        };
        self.transport_client
            .send_message(client_msg)
            .await
            .unwrap();

        // Send to ground (435 MHz).
        let gs_msg = TransportMessage {
            header: TransportHeader { dest_addr: GS_ADDR },
            payload,
        };
        self.transport_gs.send_message(gs_msg).await.unwrap();
    }

    async fn recv_msg(transport: &mut T) -> AppMessage {
        let mut buf = [0u8; MAX_TRANSPORT_CHUNK_PAYLOAD];
        let msg = transport.recv_message(&mut buf).await.unwrap();
        let app_msg: AppMessage = postcard::from_bytes(&msg.payload).unwrap();
        app_msg
    }

    async fn handle_command(&mut self, cmd: GroundCommand) {
        println!("{:?}", cmd);
        match cmd {
            GroundCommand::RequestClientData => {
                self.send_big_message().await;
            }
            _ => {}
        }
    }

    async fn send_big_message(&mut self) {
        let msg = Self::create_big_message();

        let mut buf = [0u8; MAX_TRANSPORT_CHUNK_PAYLOAD];
        let ser = postcard::to_slice(&msg, &mut buf).unwrap();
        let payload = Vec::<u8, MAX_TRANSPORT_MESSAGE_PAYLOAD>::from_slice(ser).unwrap();

        let transport_msg = TransportMessage {
            header: TransportHeader { dest_addr: GS_ADDR },
            payload,
        };

        self.transport_gs.send_message(transport_msg).await.unwrap();

        println!("[sat] [tx_gs] send big message: {:?}", msg);
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

    fn create_big_message() -> AppMessage {
        let data = Vec::from_array([7u8; 25]);
        AppMessage::ClientDataMsg(ClientData { data })
    }
}

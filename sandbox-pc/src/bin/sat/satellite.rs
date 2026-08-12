use std::time::{Duration, SystemTime, UNIX_EPOCH};

use heapless::Vec;
use sandbox_pc::addr::{CLIENT_ADDR, GS_ADDR, SAT_ADDR};
use sat_core::layer::app::AppMessage;
use sat_core::layer::transport::{TransportLayer, TransportMessage};
use sat_core::message::{Beacon, ClientData, GroundCommand, SatelliteData};

pub struct Satellite<T: TransportLayer> {
    /// Transport to ground station (435 MHz).
    transport_gs: T,
    /// Transport to client device (868 MHz).
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
            let rx_gs = Self::receive_gs_cmd(&mut self.transport_gs);
            let rx_client = Self::receive_client_data(&mut self.transport_client);

            tokio::select! {
                data = rx_client => {
                    match data {
                        Some(msg) => println!("[sat] rx [868] data: {msg:?}"),
                        None => eprintln!("[sat] rx [868] no data"),
                    }
                }

                cmd = rx_gs => {
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

        let mut buf = [0u8; 4096];
        let msg = AppMessage::BeaconMsg(beacon);
        let ser = postcard::to_slice(&msg, &mut buf).unwrap();
        let payload = Vec::<u8, 4096>::from_slice(ser).unwrap();

        // Send to client (868 MHz).
        let client_msg = TransportMessage {
            payload: payload.clone(),
            dest_addr: CLIENT_ADDR,
        };
        self.transport_client
            .try_send_message(client_msg)
            .await
            .unwrap();

        // Send to ground (435 MHz).
        let gs_msg = TransportMessage {
            payload,
            dest_addr: GS_ADDR,
        };
        self.transport_gs.try_send_message(gs_msg).await.unwrap();
    }

    async fn receive_client_data(transport: &mut T) -> Option<ClientData> {
        let mut buf = [0u8; 4096];
        let msg = transport.try_recv_message(&mut buf).await.ok()?;
        let app_msg: AppMessage = postcard::from_bytes(&msg.payload).ok()?;
        match app_msg {
            AppMessage::ClientDataMsg(client_data) => Some(client_data),
            _ => None,
        }
    }

    async fn receive_gs_cmd(transport: &mut T) -> Option<GroundCommand> {
        let mut buf = [0u8; 4096];
        let msg = transport.try_recv_message(&mut buf).await.ok()?;
        let app_msg: AppMessage = postcard::from_bytes(&msg.payload).ok()?;
        match app_msg {
            AppMessage::GndCommandMsg(cmd) => Some(cmd),
            _ => None,
        }
    }

    async fn handle_command(&mut self, cmd: GroundCommand) {
        match cmd {
            GroundCommand::RequestTelemetry => {
                self.send_tm().await;
            }
            _ => {}
        }
    }

    async fn send_tm(&mut self) {
        let data = SatelliteData {
            data: Vec::from_slice(&[0, 1, 2, 3, 4, 5, 6, 7]).unwrap(),
        };
        let msg = AppMessage::SatDataMsg(data);

        let mut buf = [0u8; 4096];
        let ser = postcard::to_slice(&msg, &mut buf).unwrap();
        let payload = Vec::<u8, 4096>::from_slice(ser).unwrap();

        let transport_msg = TransportMessage {
            payload,
            dest_addr: GS_ADDR,
        };
        println!("[sat] tx telemetry");
        self.transport_gs
            .try_send_message(transport_msg)
            .await
            .unwrap();
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

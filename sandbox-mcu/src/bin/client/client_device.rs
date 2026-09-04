use defmt::info;
use embassy_futures::select::{Either, select};
use embassy_time::Duration;

use heapless::Vec;
use sat_core::layer::app::Message;
use sat_core::layer::transport::{self, Packet, PacketHeader, TransportLayer};
use sat_core::message::beacon::Beacon;
use sat_core::message::client_data::{self, ClientData};

use super::telemetry::{self, Telemetry, TelemetryVec};

pub struct ClientDevice<T: TransportLayer> {
    transport: T,
    pending_tm: TelemetryVec,
    counter: u32,
}

impl<T: TransportLayer> ClientDevice<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            pending_tm: Vec::new(),
            counter: 0,
        }
    }

    pub async fn run(mut self) {
        let mut tm_ticker = embassy_time::Ticker::every(Duration::from_millis(3777));

        info!("client device online");

        loop {
            match select(self.wait_beacon(), tm_ticker.next()).await {
                Either::First(beacon) => self.on_beacon(beacon).await,
                Either::Second(_) => {
                    self.make_tm();
                }
            };
        }
    }

    async fn on_beacon(&mut self, beacon: Beacon) {
        self.flush_tm(beacon.sat_addr).await
    }

    async fn wait_beacon(&mut self) -> Beacon {
        info!("wait for beacon");
        loop {
            let mut buf = [0u8; 4096];
            let msg = self.transport.recv_message(&mut buf).await.unwrap();
            if let Ok(Message::BeaconMsg(beacon)) = postcard::from_bytes(&msg.payload) {
                return beacon;
            }
        }
    }

    async fn flush_tm(&mut self, sat_addr: u32) {
        if self.pending_tm.is_empty() {
            return;
        }

        let mut tm_buf = [0u8; client_data::MAX_CLIENT_DATA];
        let tm_slice = telemetry::encode_vec(&self.pending_tm, &mut tm_buf);
        let data = Vec::<u8, { client_data::MAX_CLIENT_DATA }>::from_slice(tm_slice).unwrap();

        let msg = Message::ClientDataMsg(ClientData { data });

        let mut buf = [0u8; transport::MAX_TRANSPORT_MESSAGE_PAYLOAD];
        let ser = postcard::to_slice(&msg, &mut buf).unwrap();
        let payload =
            Vec::<u8, { transport::MAX_TRANSPORT_MESSAGE_PAYLOAD }>::from_slice(ser).unwrap();

        let transport_msg = Packet {
            header: PacketHeader {
                dest_addr: sat_addr,
            },
            payload,
        };

        info!("tx telemetry ({} samples) ...", self.pending_tm.len());
        self.transport.send_message(transport_msg).await.unwrap();
        info!("tx done");
        self.pending_tm.clear();
    }

    fn make_tm(&mut self) {
        let tm = Telemetry {
            temp: 451.1,
            pressure: 102.5,
            bat_voltage: 4.8,
            id: self.counter,
        };
        self.counter += 1;
        info!("make tm: {:?}", tm);
        self.pending_tm.push(tm).unwrap();
    }
}

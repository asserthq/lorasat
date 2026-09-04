use defmt::*;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Ticker};
use heapless::Vec;
use sat_core::layer::app::Message;
use sat_core::layer::transport::{
    MAX_TRANSPORT_CHUNK_PAYLOAD, MAX_TRANSPORT_MESSAGE_PAYLOAD, Packet, PacketHeader,
    TransportLayer,
};
use sat_core::message::Beacon;

pub struct Satellite<T: TransportLayer> {
    comm_gs: Option<T>,
    comm_client: T,
}

impl<T: TransportLayer> Satellite<T> {
    pub fn new(comm_gs: Option<T>, comm_client: T) -> Self {
        Self {
            comm_gs,
            comm_client,
        }
    }

    pub async fn run(mut self) {
        let mut beacon_ticker = Ticker::every(Duration::from_secs(10));
        let beacon = Beacon {
            sat_addr: 1,
            interval_sec: 10,
            timestamp: 777,
        };

        info!("sat online, listening");

        loop {
            //let rx_gs = Self::recv_msg(&mut self.comm_gs);
            let rx_client = Self::recv_msg(&mut self.comm_client);

            match select(rx_client, beacon_ticker.next()).await {
                Either::First(_msg) => {
                    info!("rx msg from client");
                }
                Either::Second(_) => {
                    self.send_beacon(beacon.clone()).await;
                }
            }
        }
    }

    async fn send_beacon(&mut self, beacon: Beacon) {
        let mut buf = [0u8; MAX_TRANSPORT_CHUNK_PAYLOAD];
        let msg = Message::BeaconMsg(beacon);
        let ser = postcard::to_slice(&msg, &mut buf).unwrap();
        let payload = Vec::<u8, MAX_TRANSPORT_MESSAGE_PAYLOAD>::from_slice(ser).unwrap();

        let client_msg = Packet {
            header: PacketHeader { dest_addr: 2 },
            payload: payload.clone(),
        };
        self.comm_client.send_message(client_msg).await.unwrap();

        info!("sent beacon CLIENT");

        // Send to ground (435 MHz).
        if let Some(comm_gs) = &mut self.comm_gs {
            let gs_msg = Packet {
                header: PacketHeader { dest_addr: 0 },
                payload,
            };
            comm_gs.send_message(gs_msg).await.unwrap();
        }

        info!("sent beacon GS");
    }

    async fn recv_msg(comm: &mut T) -> Message {
        let mut buf = [0u8; MAX_TRANSPORT_CHUNK_PAYLOAD];
        let pkt = comm.recv_message(&mut buf).await.unwrap();
        let msg: Message = postcard::from_bytes(&pkt.payload).unwrap();
        msg
    }
}

use core::time::Duration;
use sandbox_pc::addr::SAT_ADDR;
use sat_core::layer::app::Message;
use sat_core::layer::transport::{
    MAX_TRANSPORT_CHUNK_PAYLOAD, MAX_TRANSPORT_MESSAGE_PAYLOAD, Packet, PacketHeader,
    TransportLayer,
};
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
        let mut request_interval = tokio::time::interval(Duration::from_secs(10));

        loop {
            let rx_sat = Self::recv_msg(&mut self.transport);

            select! {
                sat_msg = rx_sat => {
                    print!("[gs] [rx_sat] ");
                    match sat_msg {
                        Message::BeaconMsg(beacon) => println!("{:?}", beacon),
                        Message::ClientDataMsg(client_data) => println!("{:?}", client_data),
                        msg => println!("nonsense: {:?}", msg)
                    }
                }

                _ = request_interval.tick() => {
                    self.request_data().await;
                }
            }
        }
    }

    async fn request_data(&mut self) {
        let cmd = GroundCommand::RequestClientData;
        let msg = Message::GndCommandMsg(cmd);

        let mut buf = [0u8; MAX_TRANSPORT_CHUNK_PAYLOAD];
        let ser = postcard::to_slice(&msg, &mut buf).unwrap();
        let payload = Vec::<u8, MAX_TRANSPORT_MESSAGE_PAYLOAD>::from_slice(ser).unwrap();

        // dest_addr flows through the message
        let transport_msg = Packet {
            header: PacketHeader {
                dest_addr: SAT_ADDR,
            },
            payload,
        };

        self.transport.send_message(transport_msg).await.unwrap();
        println!("[gs] [tx_sat] request client data");
    }

    async fn wait_beacon(&mut self) -> Option<Beacon> {
        let mut buf = [0u8; 4096];
        let transport_msg = self.transport.recv_message(&mut buf).await.unwrap();

        let app_msg: Message = postcard::from_bytes(&transport_msg.payload).unwrap();

        match app_msg {
            Message::BeaconMsg(beacon) => Some(beacon),
            _ => None,
        }
    }

    async fn recv_msg(transport: &mut T) -> Message {
        let mut buf = [0u8; MAX_TRANSPORT_CHUNK_PAYLOAD];
        let transport_msg = transport.recv_message(&mut buf).await.unwrap();
        let app_msg: Message = postcard::from_bytes(&transport_msg.payload).unwrap();
        app_msg
    }
}

use defmt::info;
use embassy_time::Timer;
use heapless::Vec;

use sat_core::layer::app::Message;
use sat_core::layer::transport::{self, Packet, PacketHeader, TransportLayer};
use sat_core::message::client_data::{ClientData, MAX_CLIENT_DATA};
use sat_core::message::{Beacon, Telemetry};

pub async fn wait_beacon<T: TransportLayer>(transport: &mut T) -> Beacon {
    let mut buf = [0u8; transport::MAX_TRANSPORT_MESSAGE_PAYLOAD];

    loop {
        let pkt = match transport.recv_message(&mut buf).await {
            Ok(pkt) => pkt,
            Err(_) => {
                Timer::after_millis(100).await;
                continue;
            }
        };

        match postcard::from_bytes::<Message>(&pkt.payload) {
            Ok(Message::BeaconMsg(beacon)) => return beacon,
            Ok(_) => info!("ignored non-beacon message"),
            Err(_) => info!("decode failed"),
        }
    }
}

pub async fn send_client_telemetry<T: TransportLayer>(
    transport: &mut T,
    dest: u32,
    tm: &Telemetry,
) {
    let mut data_buf = [0u8; MAX_CLIENT_DATA];
    let tm_ser = postcard::to_slice(tm, &mut data_buf).unwrap();

    let msg = Message::ClientDataMsg(ClientData {
        data: Vec::<u8, MAX_CLIENT_DATA>::from_slice(tm_ser).unwrap(),
    });

    let mut buf = [0u8; transport::MAX_TRANSPORT_MESSAGE_PAYLOAD];
    let ser = postcard::to_slice(&msg, &mut buf).unwrap();
    let payload = Vec::<u8, { transport::MAX_TRANSPORT_MESSAGE_PAYLOAD }>::from_slice(ser).unwrap();

    let pkt = Packet {
        header: PacketHeader { dest_addr: dest },
        payload,
    };

    info!("tx ClientDataMsg -> 0x{:08x}: {:?}", dest, tm);
    transport.send_message(pkt).await.unwrap();
    info!("tx done");
}

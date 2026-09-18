use defmt::*;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Ticker};
use heapless::Vec;
use postcard::from_bytes;
use sat_core::layer::app::Message;
use sat_core::layer::transport::{self, Packet, PacketHeader, TransportLayer};
use sat_core::message::{Beacon, ClientData, Command};
use sat_core::storage::Logger;

async fn recv_msg<T: TransportLayer>(transport: &mut T) -> Message {
    let mut buf = [0u8; { transport::MAX_TRANSPORT_MESSAGE_PAYLOAD }];
    let pkt = transport.recv_message(&mut buf).await.unwrap();
    info!("packet recieved: {:?}", pkt);

    let msg = from_bytes::<Message>(&pkt.payload).unwrap();
    msg
}

async fn send_msg<T: TransportLayer>(msg: Message, transport: &mut T) {
    let mut buf = [0u8; { transport::MAX_TRANSPORT_MESSAGE_PAYLOAD }];
    let ser = postcard::to_slice(&msg, &mut buf).unwrap();
    let payload = Vec::<u8, { transport::MAX_TRANSPORT_MESSAGE_PAYLOAD }>::from_slice(ser).unwrap();
    let pkt = Packet {
        header: PacketHeader { dest_addr: 0 },
        payload: payload,
    };

    info!("sending packet: {:?}", pkt);
    transport.send_message(pkt).await.unwrap();
}

async fn on_gnd_command<T: TransportLayer>(cmd: Command, transport: &mut T) {
    info!("applying ground command: {:?}", cmd);
    send_msg(Message::GroundCommandAns, transport).await;
}

async fn on_client_data(data: ClientData) {
    info!("saving client data: {:?}", data);
}

pub async fn sat_task<L, T>(mut transport: T, mut logger: L)
where
    T: TransportLayer,
    L: Logger,
    <L as Logger>::Error: defmt::Format,
{
    info!("satellite online");

    let mut beacon_ticker = Ticker::every(Duration::from_secs(10));
    let beacon = Beacon {
        sat_addr: 1,
        interval_sec: 10,
        timestamp: 777,
    };

    loop {
        let rx_msg = recv_msg(&mut transport);

        match select(rx_msg, beacon_ticker.next()).await {
            Either::First(msg) => {
                info!("recieved message: {:?}", msg);
                match msg {
                    Message::GndCommandMsg(cmd) => on_gnd_command(cmd, &mut transport).await,
                    Message::ClientDataMsg(data) => on_client_data(data).await,
                    _ => {}
                };
            }
            Either::Second(_) => {
                info!("sending beacon: {:?}", beacon);
                send_msg(Message::BeaconMsg(beacon.clone()), &mut transport).await;
            }
        }

        // match phy.recv_bytes(&mut buf).await {
        //     Ok(recv) => {
        //         info!("-> {}", recv);
        //         let mut frame = [0u8; 256];
        //         frame[0] = recv.len() as u8;
        //         frame[1..1 + recv.len()].copy_from_slice(recv);

        //         match logger.append(&frame[..1 + recv.len()]) {
        //             Ok(()) => info!("sd: logged {} bytes", recv.len()),
        //             Err(e) => error!("sd write failed: {}", e),
        //         }

        //         match postcard::from_bytes::<GroundCommand>(recv) {
        //             Ok(cmd) => info!("cmd received: {:?}", cmd),
        //             Err(_) => error!("decode failed"),
        //         }
        //     }
        //     Err(_) => error!("recv failed"),
        // }
    }
}

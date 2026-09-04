use defmt::{error, info};
use embedded_io_async::{Read, Write};
use heapless::Vec;
use sandbox_lib::shell::Shell;
use sat_core::{
    layer::{
        app::Message,
        transport::{self, Packet, PacketHeader, TransportLayer},
    },
    message::GroundCommand,
};

use crate::commands;

async fn send_cmd<T: TransportLayer>(cmd: GroundCommand, transport: &mut T) {
    let msg = Message::GndCommandMsg(cmd);
    let mut buf = [0u8; { transport::MAX_TRANSPORT_MESSAGE_PAYLOAD }];
    let ser = postcard::to_slice(&msg, &mut buf).unwrap();
    let payload = Vec::<u8, { transport::MAX_TRANSPORT_MESSAGE_PAYLOAD }>::from_slice(ser).unwrap();
    let pkt = Packet {
        header: PacketHeader { dest_addr: 1 },
        payload: payload,
    };
    info!("sending packet: {:?}", pkt);
    transport.send_message(pkt).await.unwrap();
    info!("packet sent");
}

pub async fn gs_task<U, T>(mut shell: Shell<U>, mut transport: T)
where
    U: Read + Write,
    T: TransportLayer,
{
    info!("gs online");
    shell.inner_mut().write_all(b"gs> ").await.unwrap();

    let mut line = [0u8; 128];

    loop {
        let len = match shell.read_line(&mut line).await {
            Ok(len) => len,
            Err(e) => {
                error!("read_line failed: {:?}", e);
                continue;
            }
        };

        if len == 0 {
            shell.inner_mut().write_all(b"gs> ").await.unwrap();
            continue;
        }

        match commands::parse(&line[..len]) {
            Some(cmd) => {
                info!("sending command: {:?}", cmd);
                send_cmd(cmd, &mut transport).await;
                info!("command sent");
            }
            None => {
                shell
                    .inner_mut()
                    .write_all(b"unknown command\r\n")
                    .await
                    .unwrap();
            }
        }

        shell.inner_mut().write_all(b"gs> ").await.unwrap();
    }
}

// pub async fn gs_task<U, P>(mut shell: Shell<U>, mut radio: P)
// where
//     U: Read + Write,
//     P: PhyLayer,
//     <P as PhyLayer>::Error: defmt::Format,
// {
//     info!("gs online");
//     shell.inner_mut().write_all(b"gs> ").await.unwrap();

//     let mut line = [0u8; 128];

//     loop {
//         let len = match shell.read_line(&mut line).await {
//             Ok(len) => len,
//             Err(e) => {
//                 error!("read_line failed: {:?}", e);
//                 continue;
//             }
//         };

//         if len == 0 {
//             shell.inner_mut().write_all(b"gs> ").await.unwrap();
//             continue;
//         }

//         match commands::parse(&line[..len]) {
//             Some(cmd) => {
//                 let mut buf = [0u8; 64];
//                 match postcard::to_slice(&cmd, &mut buf) {
//                     Ok(ser) => match radio.send_bytes(ser).await {
//                         Ok(()) => info!("tx {:?}", cmd),
//                         Err(e) => error!("tx failed: {:?}", e),
//                     },
//                     Err(_) => error!("serialize failed"),
//                 }
//             }
//             None => {
//                 shell
//                     .inner_mut()
//                     .write_all(b"unknown command\r\n")
//                     .await
//                     .unwrap();
//             }
//         }

//         shell.inner_mut().write_all(b"gs> ").await.unwrap();
//     }
// }

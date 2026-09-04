use defmt::*;
use sat_core::layer::phy::PhyLayer;
use sat_core::message::GroundCommand;
use sat_core::storage::Logger;

fn command_reply(cmd: &GroundCommand) -> &'static [u8] {
    match cmd {
        GroundCommand::RequestSatTelemetry => b"executed: RequestSatTelemetry",
        GroundCommand::RequestClientData => b"executed: RequestClientData",
        GroundCommand::ChangeBeaconInterval(_) => b"executed: ChangeBeaconInterval",
        GroundCommand::SetTime(_) => b"executed: SetTime",
        GroundCommand::Ping => b"executed: Ping",
    }
}

pub async fn sat_task<L>(mut phy: impl PhyLayer, mut logger: L)
where
    L: Logger,
    <L as Logger>::Error: defmt::Format,
{
    loop {
        info!("listening...");
        let mut buf = [0u8; 255];
        match phy.recv_bytes(&mut buf).await {
            Ok(recv) => {
                info!("-> {}", recv);
                let mut frame = [0u8; 256];
                frame[0] = recv.len() as u8;
                frame[1..1 + recv.len()].copy_from_slice(recv);

                match logger.append(&frame[..1 + recv.len()]) {
                    Ok(()) => info!("sd: logged {} bytes", recv.len()),
                    Err(e) => error!("sd write failed: {}", e),
                }

                match postcard::from_bytes::<GroundCommand>(recv) {
                    Ok(cmd) => {
                        info!("cmd received: {:?}", cmd);
                        let reply = command_reply(&cmd);
                        match phy.send_bytes(reply).await {
                            Ok(()) => info!("reply sent: {}", reply),
                            Err(_) => error!("reply send failed"),
                        }
                    }
                    Err(_) => error!("decode failed"),
                }
            }
            Err(_) => error!("recv failed"),
        }
    }
}

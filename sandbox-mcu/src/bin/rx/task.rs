use defmt::*;
use sat_core::layer::phy::PhyLayer;
use sat_core::storage::Logger;

pub async fn rx_task<L>(mut phy: impl PhyLayer, mut logger: L)
where
    L: Logger,
    <L as Logger>::Error: defmt::Format,
{
    loop {
        info!("receiving...");
        let mut buf = [0u8; 255];
        match phy.recv_bytes(&mut buf).await {
            Ok(recv) => {
                defmt::info!("-> {}", recv);
                let mut frame = [0u8; 256];
                frame[0] = recv.len() as u8;
                frame[1..1 + recv.len()].copy_from_slice(recv);

                match logger.append(&frame[..1 + recv.len()]) {
                    Ok(()) => defmt::info!("sd: logged {} bytes", recv.len()),
                    Err(e) => defmt::error!("sd write failed: {}", e),
                }
            }
            Err(_) => defmt::error!("receive failed"),
        }
    }
}

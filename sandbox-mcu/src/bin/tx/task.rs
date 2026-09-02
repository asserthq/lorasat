use defmt::*;
use embassy_time::{Duration, Timer};
use sat_core::layer::phy::PhyLayer;

pub async fn tx_task<P: PhyLayer>(mut phy: P) {
    loop {
        info!("sending...");
        let buf = b"helo";
        match phy.send_bytes(buf).await {
            Ok(()) => defmt::info!("<- {}", buf),
            Err(_) => defmt::error!("send failed"),
        }
        Timer::after(Duration::from_millis(2000)).await;
    }
}

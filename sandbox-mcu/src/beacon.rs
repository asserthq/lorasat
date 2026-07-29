use defmt::*;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use lora_phy::LoRa;
use lora_phy::mod_params::*;
use {defmt_rtt as _, panic_probe as _};

const LORA_FREQUENCY_IN_HZ: u32 = 433_000_000; // warning: set this appropriately for the region

#[embassy_executor::task]
async fn beacon_task(lora: LoRa, interval: Duration) {
    let mdltn_params = {
        match lora.create_modulation_params(
            SpreadingFactor::_10,
            Bandwidth::_250KHz,
            CodingRate::_4_8,
            LORA_FREQUENCY_IN_HZ,
        ) {
            Ok(mp) => mp,
            Err(err) => {
                info!("Radio error = {}", err);
                return;
            }
        }
    };

    let mut tx_pkt_params = {
        match lora.create_tx_packet_params(4, false, true, false, &mdltn_params) {
            Ok(pp) => pp,
            Err(err) => {
                info!("Radio error = {}", err);
                return;
            }
        }
    };

    let buffer = [0x01u8, 0x02u8, 0x03u8];

    loop {
        match lora
            .prepare_for_tx(&mdltn_params, &mut tx_pkt_params, 15, &buffer)
            .await
        {
            Ok(()) => {}
            Err(err) => {
                info!("Radio error = {}", err);
                return;
            }
        };

        match lora.tx().await {
            Ok(()) => {
                info!("TX DONE");
            }
            Err(err) => {
                info!("Radio error = {}", err);
                return;
            }
        };

        match lora.sleep(false).await {
            Ok(()) => info!("Sleep successful"),
            Err(err) => info!("Sleep unsuccessful = {}", err),
        }

        Timer::after(interval).await;
    }
}

#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::{Delay, Instant, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::{
    LoRa,
    iv::GenericSx126xInterfaceVariant,
    mod_params::{Bandwidth, CodingRate, SpreadingFactor},
    sx126x::{self, Sx126x, Sx1262, TcxoCtrlVoltage},
};
use {defmt_rtt as _, panic_probe as _};

const FREQUENCY_HZ: u32 = 433_000_000;
const TX_POWER_DBM: i32 = 2;
const SEND_INTERVAL_SECONDS: u64 = 2;
const TEST_PAYLOAD: &[u8] = b"raw LoRa test";

bind_interrupts!(struct Irqs {
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>; // SPI1 RX
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI1 TX
    EXTI2 => exti::InterruptHandler<interrupt::typelevel::EXTI2>; // DIO1
    EXTI1 => exti::InterruptHandler<interrupt::typelevel::EXTI1>; // BUSY
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut clock_config = embassy_stm32::Config::default();
    clock_config.rcc.hsi = true;
    clock_config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(clock_config);

    let nss = Output::new(p.PA4, Level::High, Speed::Low);
    let reset = Output::new(p.PB0, Level::High, Speed::Low);
    let busy = ExtiInput::new(p.PB1, p.EXTI1, Pull::Up, Irqs);
    let dio1 = ExtiInput::new(p.PB2, p.EXTI2, Pull::Up, Irqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH2, Irqs, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    let interface = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None).unwrap();
    let radio_config = sx126x::Config {
        chip: Sx1262,
        tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V8),
        use_dcdc: true,
        rx_boost: true,
    };
    let mut lora = LoRa::new(Sx126x::new(spi, interface, radio_config), false, Delay)
        .await
        .unwrap();
    let modulation = lora
        .create_modulation_params(
            SpreadingFactor::_12,
            Bandwidth::_125KHz,
            CodingRate::_4_5,
            FREQUENCY_HZ,
        )
        .unwrap();
    let mut tx_params = lora
        .create_tx_packet_params(8, false, true, false, &modulation)
        .unwrap();

    info!("SX1262 raw TX initialized at {} Hz", FREQUENCY_HZ);

    loop {
        let started_at = Instant::now();
        let result = async {
            lora.prepare_for_tx(&modulation, &mut tx_params, TX_POWER_DBM, TEST_PAYLOAD)
                .await?;
            lora.tx().await
        }
        .await;

        match result {
            Ok(()) => info!(
                "TX length={} elapsed_ms={} bytes={:?}",
                TEST_PAYLOAD.len(),
                started_at.elapsed().as_millis(),
                TEST_PAYLOAD
            ),
            Err(error) => error!("TX failed: {:?}", error),
        }

        Timer::after_secs(SEND_INTERVAL_SECONDS).await;
    }
}

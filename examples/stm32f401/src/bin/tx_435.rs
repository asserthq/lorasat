#![no_std]
#![no_main]

use defmt::{error, info};

use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;

use sat_core::comm::phy::PhyLayer;
use sat_drivers::lora::Radio;

use {defmt_rtt as _, panic_probe as _};

const LORA_FREQ: u32 = 435_000_000;

bind_interrupts!(struct Irqs {
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>; // SPI1 RX
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI1 TX
    EXTI1 => exti::InterruptHandler<interrupt::typelevel::EXTI1>; // DIO0
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    info!("435_tx start (SX1276, 435 MHz)");

    let nss = Output::new(p.PA4, Level::High, Speed::Low);
    let reset = Output::new(p.PB0, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PB1, p.EXTI1, Pull::Up, Irqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH2, Irqs, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    let mut radio = match Radio::new(spi, reset, irq, Delay, LORA_FREQ).await {
        Ok(r) => r,
        Err(e) => {
            error!("radio init failed: {:?}", e);
            return;
        }
    };
    info!("radio init OK");

    let payload: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];
    let mut counter: u32 = 0;

    loop {
        info!("tx packet {}: {:?}", counter, payload);
        if let Err(e) = radio.send_bytes(&payload).await {
            error!("tx failed: {:?}", e);
        }
        counter += 1;
        Timer::after_secs(3).await;
    }
}

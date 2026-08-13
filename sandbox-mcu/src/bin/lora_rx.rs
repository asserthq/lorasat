#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::{Delay, Duration, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;

use sandbox_lib as _;

use sat_core::layer::physical::PhysicalLayer;
use sat_drivers::lora::Radio;

const LORA_FREQ_IN_HZ: u32 = 435_100_000;

bind_interrupts!(struct Irqs {
    // SPI1 => spi::InterruptHandler<peripheral::SPI1>;
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>; // SPI TX
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>; // SPI RX
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // init stm32

    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    // init SPI

    let nss = Output::new(p.PB12, Level::High, Speed::Low);
    let reset = Output::new(p.PA8, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PA9, p.EXTI9, Pull::Up, Irqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
    );

    let spi_device = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    // init lora radio

    let mut radio = Radio::new(spi_device, reset, irq, Delay, LORA_FREQ_IN_HZ)
        .await
        .unwrap();

    // main loop

    loop {
        info!("receiving...");
        let mut buf = [0u8; 255];
        match radio.try_recv_bytes(&mut buf).await {
            Ok(buf) => defmt::info!("-> {}", buf),
            Err(_) => defmt::error!("receive failed"),
        }
    }
}

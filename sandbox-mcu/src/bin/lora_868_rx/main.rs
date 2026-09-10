#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use sat_core::layer::phy::PhyLayer;
use sat_drivers::lora::Radio1262;

use sandbox_lib as _;

const LORA_FREQ_IN_HZ: u32 = 868_100_000;

bind_interrupts!(struct Irqs {
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>; // SPI TX
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>; // SPI RX
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>; // DIO1
    EXTI0 => exti::InterruptHandler<interrupt::typelevel::EXTI0>;     // BUSY
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    info!("sx1262 rx: E22-900M30S init");

    let nss = Output::new(p.PB12, Level::High, Speed::Low);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(2000);
    let spi = spi::Spi::new(
        p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
    );
    let spi_device = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    let reset = Output::new(p.PA8, Level::High, Speed::Low);
    let dio1 = ExtiInput::new(p.PA9, p.EXTI9, Pull::Down, Irqs);
    let busy = ExtiInput::new(p.PB0, p.EXTI0, Pull::Down, Irqs);

    let rxen = Output::new(p.PB1, Level::Low, Speed::Low);
    let txen = Output::new(p.PA4, Level::Low, Speed::Low);

    let mut radio = Radio1262::new(
        spi_device,
        reset,
        dio1,
        busy,
        Some(rxen),
        Some(txen),
        Delay,
        LORA_FREQ_IN_HZ,
    )
    .await
    .unwrap();

    info!("sx1262 rx ready, freq {} Hz", LORA_FREQ_IN_HZ);

    let mut buf = [0u8; 255];

    loop {
        match radio.recv_bytes(&mut buf).await {
            Ok(recv) => info!("rx {} bytes: {=[u8]}", recv.len(), &recv[..]),
            Err(e) => error!("rx failed: {}", e),
        }
    }
}

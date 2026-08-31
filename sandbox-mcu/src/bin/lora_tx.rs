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

const LORA_FREQ_IN_HZ: u32 = 433_000_000;

bind_interrupts!(struct Irqs {
    // SPI1 => spi::InterruptHandler<peripheral::SPI1>;
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI TX
    DMA2_STREAM0 => dma::InterruptHandler<peripherals::DMA2_CH0>; // SPI RX
    EXTI0 => exti::InterruptHandler<interrupt::typelevel::EXTI0>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // init stm32

    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    // init SPI

    let nss = Output::new(p.PA4, Level::High, Speed::Low);
    let reset = Output::new(p.PB1, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PB0, p.EXTI0, Pull::Up, Irqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH0, Irqs, spi_config,
    );

    let spi_device = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    // init lora radio

    let mut radio = Radio::new(spi_device, reset, irq, Delay, LORA_FREQ_IN_HZ)
        .await
        .unwrap();

    // main loop

    loop {
        info!("sending...");
        let buf = b"helo";
        match radio.send_bytes(buf).await {
            Ok(()) => defmt::info!("<- {}", buf),
            Err(_) => defmt::error!("send failed"),
        }
        Timer::after(Duration::from_millis(2000)).await;
    }
}

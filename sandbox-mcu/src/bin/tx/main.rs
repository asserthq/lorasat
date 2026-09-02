#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{Peripherals, bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use sandbox_lib as _;

use sat_core::layer::phy::PhyLayer;
use sat_drivers::lora::Radio;

mod task;

const LORA_FREQ_IN_HZ: u32 = 433_000_000;

bind_interrupts!(struct Irqs {
    // SPI1 => spi::InterruptHandler<peripheral::SPI1>;
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI TX
    DMA2_STREAM0 => dma::InterruptHandler<peripherals::DMA2_CH0>; // SPI RX
    EXTI0 => exti::InterruptHandler<interrupt::typelevel::EXTI0>;
});

async fn init_peri() -> Peripherals {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    embassy_stm32::init(config)
}

async fn init_radio(p: Peripherals) -> impl PhyLayer {
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

    Radio::new(spi_device, reset, irq, Delay, LORA_FREQ_IN_HZ)
        .await
        .unwrap()
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = init_peri().await;
    let phy = init_radio(p).await;
    task::tx_task(phy).await;
}

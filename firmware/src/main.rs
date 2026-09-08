#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Input, Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{Peripherals, bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use firmware_lib as _;

use sat_drivers::lora::Radio;
use sat_drivers::proto_impl::link::LinkImpl;
use sat_drivers::proto_impl::transport::simple::SimpleTransport;

use crate::sat::Satellite;

mod adcs;
mod comm;
mod sat;

const LORA_FREQ_IN_HZ: u32 = 433_000_000;

bind_interrupts!(struct Irqs {
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>; // SPI TX
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>; // SPI RX
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;
});

async fn init_peri() -> Peripherals {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    embassy_stm32::init(config)
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = init_peri().await;

    let phy_client = {
        let nss = Output::new(p.PB12, Level::High, Speed::Low);
        let reset = Output::new(p.PA8, Level::High, Speed::Low);
        let irq = ExtiInput::new(p.PA9, p.EXTI9, Pull::Up, Irqs);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(200);
        let spi = spi::Spi::new(
            p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
        );

        let spi_device = ExclusiveDevice::new(spi, nss, Delay).unwrap();

        Radio::new(spi_device, reset, irq, Delay, LORA_FREQ_IN_HZ)
            .await
            .unwrap()
    };

    let link_client = LinkImpl::new(phy_client);
    let ts_client = SimpleTransport::new(1, link_client);

    let sat = Satellite::new(None, ts_client);
}

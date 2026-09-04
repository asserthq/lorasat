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
use sat_drivers::lora::Radio;
use sat_drivers::proto_impl::link::LinkImpl;
use sat_drivers::proto_impl::transport::SimpleTransport;

mod client_device;
mod telemetry;

const LORA_FREQ_IN_HZ: u32 = 433_000_000;

bind_interrupts!(struct Irqs {
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>;
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>;
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;
});

fn init_peri() -> Peripherals {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    embassy_stm32::init(config)
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = init_peri();

    let phy = {
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

    let link = LinkImpl::new(phy);
    let transport = SimpleTransport::new(10, link);

    let device = client_device::ClientDevice::new(transport);

    device.run().await;
}

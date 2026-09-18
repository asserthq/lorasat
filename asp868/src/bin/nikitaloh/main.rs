#![no_std]
#![no_main]

mod app;
mod identity;
mod led;
mod link;

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use sat_drivers::lora::Radio1262;
use sat_drivers::proto_impl::link::LinkImpl;
use sat_drivers::proto_impl::transport::SimpleTransport;

use {defmt_rtt as _, panic_probe as _};

const LORA_FREQ: u32 = 868_000_000;

bind_interrupts!(struct Irqs {
    DMA1_CHANNEL4_5_6_7 => dma::InterruptHandler<peripherals::DMA1_CH4>,
                            dma::InterruptHandler<peripherals::DMA1_CH5>;
    EXTI4_15 => exti::InterruptHandler<interrupt::typelevel::EXTI4_15>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    identity::init_seed();

    info!("ASP ALOHA start");

    let led_g = Output::new(p.PB2, Level::High, Speed::Low);
    let led_r = Output::new(p.PB10, Level::High, Speed::Low);
    spawner.spawn(led::led_task(led_g, led_r).unwrap());

    let nss = Output::new(p.PB12, Level::High, Speed::Low);
    let reset = Output::new(p.PB8, Level::High, Speed::Low);
    let irq_dio1 = ExtiInput::new(p.PB6, p.EXTI6, Pull::Up, Irqs);
    let irq_busy = ExtiInput::new(p.PB7, p.EXTI7, Pull::Up, Irqs);

    let mut spi_cfg = spi::Config::default();
    spi_cfg.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH5, p.DMA1_CH4, Irqs, spi_cfg,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    let radio = Radio1262::new(spi, reset, irq_dio1, irq_busy, None, None, Delay, LORA_FREQ)
        .await
        .unwrap();
    info!("LoRa init OK");

    let link = LinkImpl::new(radio);
    let node_addr = identity::chip_uid();
    info!("node addr = 0x{:08x}", node_addr);
    let transport = SimpleTransport::new(node_addr, link);

    app::run(transport, node_addr).await;
}

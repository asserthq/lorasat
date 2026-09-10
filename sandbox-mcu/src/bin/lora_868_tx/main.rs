#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::{Delay, Duration, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;

use sat_core::layer::phy::PhyLayer;
use sat_drivers::lora::Radio1262;

use sandbox_lib as _;

const LORA_FREQ_IN_HZ: u32 = 868_100_000;

bind_interrupts!(struct Irqs {
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>;
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>;
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;
    EXTI0 => exti::InterruptHandler<interrupt::typelevel::EXTI0>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    info!("sx1262 test: E22-900M30S init");

    let nss = Output::new(p.PB12, Level::High, Speed::Low);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
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

    info!("sx1262 init ok, freq {} Hz", LORA_FREQ_IN_HZ);

    let mut counter: u32 = 0;

    loop {
        let mut payload = [0u8; 8];
        payload[..4].copy_from_slice(b"PING");
        payload[4..8].copy_from_slice(&counter.to_le_bytes());

        info!("tx #{} payload {=[u8]}", counter, &payload[..]);

        match radio.send_bytes(&payload).await {
            Ok(()) => info!("tx done"),
            Err(e) => error!("tx failed: {}", e),
        }

        counter = counter.wrapping_add(1);
        Timer::after(Duration::from_secs(2)).await;
    }
}

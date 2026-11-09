#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::{Delay, Duration, Timer, with_timeout};
use embedded_hal_bus::spi::ExclusiveDevice;
use sat_core::comm::PhyLayer;
use sat_drivers::lora::Radio1262;
use stm32f401_lib as _;

const FREQUENCY_HZ: u32 = 868_000_000;
const SEND_INTERVAL_SECONDS: u64 = 3;

#[derive(serde::Serialize)]
struct Packet {
    counter: u32,
    value: u16,
}

bind_interrupts!(struct Irqs {
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI1 TX
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>; // SPI1 RX
    EXTI2 => exti::InterruptHandler<interrupt::typelevel::EXTI2>; // DIO1 PB2
    EXTI1 => exti::InterruptHandler<interrupt::typelevel::EXTI1>; // BUSY PB1
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    let nss = Output::new(p.PA4, Level::High, Speed::Low);
    let reset = Output::new(p.PB6, Level::High, Speed::Low);
    let dio1 = ExtiInput::new(p.PB2, p.EXTI2, Pull::Up, Irqs);
    let busy = ExtiInput::new(p.PB1, p.EXTI1, Pull::Up, Irqs);
    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH2, Irqs, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();
    let mut radio = with_timeout(
        Duration::from_secs(10),
        Radio1262::new(spi, reset, dio1, busy, None, None, Delay, FREQUENCY_HZ),
    )
    .await
    .expect("SX1262 init timeout; check SPI1/BUSY/RESET")
    .unwrap();

    info!("Sender ready: Radio1262/SPI1, 868 MHz, SF12, TX +20 dBm, synthetic value, no ACK");
    let mut counter = 1_u32;
    let mut buffer = [0_u8; 16];
    loop {
        let packet = Packet {
            counter,
            value: 2200,
        };
        let bytes = postcard::to_slice(&packet, &mut buffer).unwrap();
        radio.send_bytes(bytes).await.unwrap();
        info!(
            "TX counter={} value={} bytes={:?}",
            counter, packet.value, bytes
        );
        counter = counter.checked_add(1).expect("test counter exhausted");
        Timer::after_secs(SEND_INTERVAL_SECONDS).await;
    }
}

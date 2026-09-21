#![no_std]
#![no_main]

mod config;
mod logger;
mod task;

use defmt::{error, info};

use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Input, Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use sat_core::comm::Address;
use sat_core::entity::Beacon;
use sat_drivers::lora::Radio1262;
use sat_drivers::proto_impl::link::LinkImpl;
use sat_drivers::sd::{SdCardLogger, StaticTimeSource};

use logger::BitbangSpiDevice;

use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>; // SPI RX
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI TX
    EXTI4 => exti::InterruptHandler<interrupt::typelevel::EXTI4>; // DIO1
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;    // BUSY
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    let radio = {
        let nss = Output::new(p.PA4, Level::High, Speed::Low);
        let reset = Output::new(p.PB6, Level::High, Speed::Low);
        let irq_dio1 = ExtiInput::new(p.PB4, p.EXTI4, Pull::Up, Irqs);
        let irq_busy = ExtiInput::new(p.PB5, p.EXTI5, Pull::Up, Irqs);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(200);
        let spi = spi::Spi::new(
            p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH2, Irqs, spi_config,
        );
        let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();

        Radio1262::new(
            spi,
            reset,
            irq_dio1,
            irq_busy,
            None,
            None,
            Delay,
            config::LORA_FREQUENCY_IN_HZ,
        )
        .await
        .unwrap()
    };

    let link = LinkImpl::new(Address(config::SAT_ADDR), radio);

    let logger = {
        let sck = Output::new(p.PB12, Level::Low, Speed::High);
        let mosi = Output::new(p.PB13, Level::Low, Speed::High);
        let miso = Input::new(p.PB14, Pull::Up);
        let nss = Output::new(p.PB15, Level::High, Speed::High);

        let spi = BitbangSpiDevice::new(sck, mosi, miso, Delay);
        let mut logger = SdCardLogger::new(spi, nss, Delay, StaticTimeSource::default());

        match logger.enable_rotation(1_000_000) {
            Ok(()) => info!("sd: rotation on, 1 MiB/file"),
            Err(e) => error!("sd rotation failed: {}", e),
        }
        logger.set_max_total_size(400_000_000);

        match logger.init() {
            Ok(()) => info!(
                "sd: card size {} bytes",
                logger.card_size_bytes().unwrap_or(0)
            ),
            Err(e) => error!("sd init failed: {}", e),
        }

        logger
    };

    let beacon = Beacon {
        sat_addr: Address(config::SAT_ADDR),
        interval_sec: config::BEACON_INTERVAL_SEC,
        timestamp: 777,
    };

    task::sat_task(link, beacon, logger).await;
}

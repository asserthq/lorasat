#![no_std]
#![no_main]

mod config;
mod flash_logger;
mod task;

use defmt::{error, info};

use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::{khz, mhz};
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use sat_core::comm::Address;
use sat_core::entity::Beacon;
use sat_drivers::flash::ring::FlashRing;
use sat_drivers::flash::{CAPACITY, W25Q64};
use sat_drivers::lora::Radio1262;
use sat_drivers::proto_impl::link::LinkImpl;

use flash_logger::FlashLogger;

use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>; // SPI1 RX (radio)
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI1 TX (radio)
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>; // SPI2 RX (flash)
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>; // SPI2 TX (flash)
    EXTI4 => exti::InterruptHandler<interrupt::typelevel::EXTI4>; // DIO1
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>; // BUSY
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
        let cs = Output::new(p.PB12, Level::High, Speed::High);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = mhz(1);

        let spi = spi::Spi::new(
            p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
        );
        let spi = ExclusiveDevice::new(spi, cs, Delay).unwrap();

        let mut flash = W25Q64::new(spi, Delay);
        match flash.read_id() {
            Ok(id) => info!("flash: JEDEC ID {:02x} {:02x} {:02x}", id[0], id[1], id[2]),
            Err(e) => error!("flash: read_id failed: {:?}", e),
        }

        let ring = FlashRing::new(flash, 0, CAPACITY).unwrap();

        let mut logger = FlashLogger::new(ring);
        match logger.recover() {
            Ok(()) => info!("flash: ring recovered"),
            Err(e) => error!("flash: recover failed: {:?}", e),
        }
        match logger.dump() {
            Ok(()) => info!("flash: dump done"),
            Err(e) => error!("flash: dump failed: {:?}", e),
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

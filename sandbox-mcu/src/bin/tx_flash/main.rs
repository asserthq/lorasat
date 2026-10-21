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
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use sat_core::comm::Address;
use sat_core::entity::Beacon;
use sat_drivers::flash::ring::FlashRing;
use sat_drivers::flash::{CAPACITY, W25Q64};
use sat_drivers::lora::{Radio, Radio1262};
use sat_drivers::proto_impl::link::LinkImpl;

use flash_logger::FlashLogger;

use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>; // SPI1 RX (radio АСП)
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI1 TX (radio АСП)
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>; // SPI2 RX (radio НС)
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>; // SPI2 TX (radio НС)
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>; // SPI3 RX (flash)
    DMA1_STREAM5 => dma::InterruptHandler<peripherals::DMA1_CH5>; // SPI3 TX (flash)
    EXTI4 => exti::InterruptHandler<interrupt::typelevel::EXTI4>; // —
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>; // BUSY (PA8), DIO0 (PB7)
    EXTI15_10 => exti::InterruptHandler<interrupt::typelevel::EXTI15_10>; // DIO1 (PB10)
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    let asp_radio = {
        let nss = Output::new(p.PA4, Level::High, Speed::Low);
        let reset = Output::new(p.PB6, Level::High, Speed::Low);
        let irq_dio1 = ExtiInput::new(p.PB10, p.EXTI10, Pull::Up, Irqs);
        let irq_busy = ExtiInput::new(p.PA8, p.EXTI8, Pull::Up, Irqs);

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

    let asp_link = LinkImpl::new(Address(config::SAT_ADDR), asp_radio);

    let gs_radio = {
        let nss = Output::new(p.PB12, Level::High, Speed::Low);
        let reset = Output::new(p.PB0, Level::High, Speed::Low);
        let irq_dio0 = ExtiInput::new(p.PB7, p.EXTI7, Pull::Up, Irqs);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(200);
        let spi = spi::Spi::new(
            p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
        );
        let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();

        Radio::new(spi, reset, irq_dio0, Delay, config::GS_FREQUENCY_IN_HZ)
            .await
            .unwrap()
    };

    let gs_link = LinkImpl::new(Address(config::SAT_ADDR), gs_radio);

    let logger = {
        let cs = Output::new(p.PB8, Level::High, Speed::High);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = mhz(1);

        let spi = spi::Spi::new(
            p.SPI3, p.PB3, p.PB5, p.PB4, p.DMA1_CH5, p.DMA1_CH0, Irqs, spi_config,
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

    let mut wdg = IndependentWatchdog::new(p.IWDG, 20_000_000);
    wdg.unleash();
    info!("watchdog: started, 20s timeout");

    task::sat_task(asp_link, gs_link, beacon, logger, wdg).await;
}

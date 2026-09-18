//! This example runs on the STM32 LoRa Discovery board, which has a builtin Semtech Sx1276 radio.
//! It demonstrates LORA P2P send functionality.
#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use heapless::Vec;
use sat_core::layer::app::Message;
use sat_core::layer::transport::{self, Packet, PacketHeader, TransportLayer};
use sat_core::message::Beacon;
use sat_drivers::lora::Radio1262;
use sat_drivers::proto_impl::link::LinkImpl;
use sat_drivers::proto_impl::transport::SimpleTransport;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>; // SPI RX
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI TX
    EXTI4 => exti::InterruptHandler<interrupt::typelevel::EXTI4>; // DIO1
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;    // BUSY
});

const LORA_FREQUENCY_IN_HZ: u32 = 868_000_000;

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
            LORA_FREQUENCY_IN_HZ,
        )
        .await
        .unwrap()
    };

    let link = LinkImpl::new(radio);
    let mut transport = SimpleTransport::new(1, link);

    let beacon = Message::BeaconMsg(Beacon {
        sat_addr: 1,
        interval_sec: 10,
        timestamp: 777,
    });

    loop {
        let mut buf = [0u8; transport::MAX_TRANSPORT_MESSAGE_PAYLOAD];
        let ser = postcard::to_slice(&beacon, &mut buf).unwrap();
        let payload =
            Vec::<u8, { transport::MAX_TRANSPORT_MESSAGE_PAYLOAD }>::from_slice(ser).unwrap();
        let pkt = Packet {
            header: PacketHeader { dest_addr: 10 },
            payload,
        };

        transport.send_message(pkt).await.unwrap();
        info!("TX DONE");
        Timer::after_secs(2).await;
    }
}
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

use sat_core::message::Beacon;
use sat_drivers::lora::Radio1262;
use sat_drivers::proto_impl::link::LinkImpl;
use sat_drivers::proto_impl::transport::SimpleTransport;
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

    let link = LinkImpl::new(radio);
    let transport = SimpleTransport::new(config::SAT_ADDR, link);

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
        sat_addr: config::SAT_ADDR,
        interval_sec: config::BEACON_INTERVAL_SEC,
        timestamp: 777,
    };

    task::sat_task(transport, beacon, logger).await;
}

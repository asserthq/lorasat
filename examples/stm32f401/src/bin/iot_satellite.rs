#![no_std]
#![no_main]

use defmt::{Debug2Format, error, info};
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::{Delay, Duration, Timer, with_timeout};
use embedded_hal_bus::spi::ExclusiveDevice;
use sat_core::comm::PhyLayer;
use sat_drivers::{
    flash::{AsyncW25Q64, CAPACITY},
    lora::Radio1262,
};
use sequential_storage::{
    cache::Cache,
    queue::{QueueConfig, QueueStorage},
};
use stm32f401_lib as _;

const FREQUENCY_HZ: u32 = 868_000_000;
const FLASH_START: u32 = 0x007F_4000;
const FLASH_END: u32 = 0x007F_E000;

#[derive(serde::Deserialize)]
struct Packet {
    counter: u32,
    value: u16,
}

bind_interrupts!(struct Irqs {
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>; // SPI1 RX
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI1 TX
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>; // SPI2 RX
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>; // SPI2 TX
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>; // SPI3 RX
    DMA1_STREAM5 => dma::InterruptHandler<peripherals::DMA1_CH5>; // SPI3 TX
    EXTI2 => exti::InterruptHandler<interrupt::typelevel::EXTI2>; // SX1262 DIO1 PB2
    EXTI1 => exti::InterruptHandler<interrupt::typelevel::EXTI1>; // SX1262 BUSY PB1
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>; // SX1268 DIO1 PA9
    EXTI15_10 => exti::InterruptHandler<interrupt::typelevel::EXTI15_10>; // BUSY PA10
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    let cs = Output::new(p.PA15, Level::High, Speed::Low);
    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(1000);
    let spi = spi::Spi::new(
        p.SPI3, p.PB3, p.PB5, p.PB4, p.DMA1_CH5, p.DMA1_CH0, Irqs, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, cs, Delay).unwrap();
    let mut flash = with_timeout(Duration::from_secs(10), AsyncW25Q64::new(spi, Delay))
        .await
        .expect("Flash init timeout; check SPI3/DMA")
        .unwrap();
    let id = flash.read_jedec_id().await.unwrap();
    info!("W25Q64 JEDEC={:02x} {:02x} {:02x}", id[0], id[1], id[2]);
    info!(
        "Flash capacity={} bytes; test queue={} bytes",
        CAPACITY,
        FLASH_END - FLASH_START
    );
    if !cfg!(feature = "iot-flash-write") {
        error!(
            "Flash writes DISABLED; reserve [{:#x}, {:#x}) and enable iot-flash-write",
            FLASH_START, FLASH_END
        );
        loop {
            Timer::after_secs(60).await;
        }
    }
    let mut queue = QueueStorage::new(
        flash,
        QueueConfig::new(FLASH_START..FLASH_END),
        Cache::new_uncached(),
    );
    let mut saved = [0_u8; 255];
    match queue.peek(&mut saved).await {
        Ok(Some(bytes)) => info!("Oldest packet already in Flash: {:?}", bytes),
        Ok(None) => info!("Flash queue is empty"),
        Err(error) => defmt::panic!("Flash queue read failed: {:?}", Debug2Format(&error)),
    }

    let _sx1268 = {
        let nss = Output::new(p.PB12, Level::High, Speed::Low);
        let reset = Output::new(p.PA8, Level::High, Speed::Low);
        let dio1 = ExtiInput::new(p.PA10, p.EXTI10, Pull::Up, Irqs);
        let busy = ExtiInput::new(p.PA9, p.EXTI9, Pull::Up, Irqs);
        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(200);
        let spi = spi::Spi::new(
            p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
        );
        let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();
        with_timeout(
            Duration::from_secs(10),
            Radio1262::new(spi, reset, dio1, busy, None, None, Delay, 435_000_000),
        )
        .await
        .expect("SX1268 init timeout; check SPI2/BUSY/RESET")
        .unwrap()
    };
    info!("SX1268/SPI2 initialized; no radio traffic on SPI2");

    let nss = Output::new(p.PA4, Level::High, Speed::Low);
    let reset = Output::new(p.PB0, Level::High, Speed::Low);
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
    let mut buffer = [0_u8; 255];
    info!("Receiver ready: Radio1262, 868 MHz, SF12, saving packets to Flash Queue");

    loop {
        let length = match radio.recv_bytes(&mut buffer).await {
            Ok(length) => length,
            Err(error) => {
                error!("LoRa receive failed: {:?}", Debug2Format(&error));
                continue;
            }
        };
        let bytes = &buffer[..length];
        let packet = match postcard::take_from_bytes::<Packet>(bytes) {
            Ok((packet, [])) => packet,
            _ => {
                error!("Invalid test packet");
                continue;
            }
        };
        info!(
            "RX counter={} value={} bytes={:?}",
            packet.counter, packet.value, bytes
        );
        match queue.push(bytes, false).await {
            Ok(()) => info!("FLASH STORED counter={}", packet.counter),
            Err(error) => error!(
                "Flash append failed; old packets retained: {:?}",
                Debug2Format(&error)
            ),
        }
    }
}

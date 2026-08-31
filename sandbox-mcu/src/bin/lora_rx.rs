#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Input, Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::Delay;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::{ErrorType, Operation, SpiDevice};
use embedded_hal_bus::spi::ExclusiveDevice;

use sandbox_lib as _;

use sat_core::layer::physical::PhysicalLayer;
use sat_drivers::lora::Radio;
use sat_drivers::sd::{SdCardLogger, StaticTimeSource};

const LORA_FREQ_IN_HZ: u32 = 433_000_000;

bind_interrupts!(struct Irqs {
    // SPI1 => spi::InterruptHandler<peripheral::SPI1>;
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI TX
    DMA2_STREAM0 => dma::InterruptHandler<peripherals::DMA2_CH0>; // SPI RX
    EXTI0 => exti::InterruptHandler<interrupt::typelevel::EXTI0>;
});

struct BitbangSpiDevice<SCK, MOSI, MISO, DELAY>
where
    SCK: OutputPin,
    MOSI: OutputPin,
    MISO: InputPin,
    DELAY: DelayNs,
{
    sck: SCK,
    mosi: MOSI,
    miso: MISO,
    delay: DELAY,
}

impl<SCK, MOSI, MISO, DELAY> BitbangSpiDevice<SCK, MOSI, MISO, DELAY>
where
    SCK: OutputPin,
    MOSI: OutputPin,
    MISO: InputPin,
    DELAY: DelayNs,
{
    fn new(sck: SCK, mosi: MOSI, miso: MISO, delay: DELAY) -> Self {
        Self {
            sck,
            mosi,
            miso,
            delay,
        }
    }

    fn transfer_byte(&mut self, byte: u8) -> u8 {
        let mut out = byte;
        let mut read = 0u8;

        for _ in 0..8 {
            if out & 0x80 != 0 {
                self.mosi.set_high().ok();
            } else {
                self.mosi.set_low().ok();
            }
            self.sck.set_high().ok();

            read = (read << 1)
                | if self.miso.is_high().unwrap_or(false) {
                    1
                } else {
                    0
                };

            self.sck.set_low().ok();
            out <<= 1;
        }

        read
    }
}

impl<SCK, MOSI, MISO, DELAY> ErrorType for BitbangSpiDevice<SCK, MOSI, MISO, DELAY>
where
    SCK: OutputPin,
    MOSI: OutputPin,
    MISO: InputPin,
    DELAY: DelayNs,
{
    type Error = core::convert::Infallible;
}

impl<SCK, MOSI, MISO, DELAY> SpiDevice<u8> for BitbangSpiDevice<SCK, MOSI, MISO, DELAY>
where
    SCK: OutputPin,
    MOSI: OutputPin,
    MISO: InputPin,
    DELAY: DelayNs,
{
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        for op in operations.iter_mut() {
            match op {
                Operation::Read(words) => {
                    for byte in words.iter_mut() {
                        *byte = self.transfer_byte(0xFF);
                    }
                }
                Operation::Write(words) => {
                    for &byte in words.iter() {
                        self.transfer_byte(byte);
                    }
                }
                Operation::Transfer(read, write) => {
                    for (r, w) in read.iter_mut().zip(write.iter()) {
                        *r = self.transfer_byte(*w);
                    }
                }
                Operation::TransferInPlace(words) => {
                    for byte in words.iter_mut() {
                        *byte = self.transfer_byte(*byte);
                    }
                }
                Operation::DelayNs(ns) => {
                    self.delay.delay_ns(*ns);
                }
            }
        }

        Ok(())
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // init stm32

    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    // init LoRa SPI (SPI2)

    let nss = Output::new(p.PA4, Level::High, Speed::Low);
    let reset = Output::new(p.PB1, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PB0, p.EXTI0, Pull::Up, Irqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH0, Irqs, spi_config,
    );

    let spi_device = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    // init lora radio

    let mut radio = Radio::new(spi_device, reset, irq, Delay, LORA_FREQ_IN_HZ)
        .await
        .unwrap();

    // init SD card
    let sd_sck = Output::new(p.PA5, Level::Low, Speed::High);
    let sd_mosi = Output::new(p.PA7, Level::Low, Speed::High);
    let sd_miso = Input::new(p.PA6, Pull::Up);
    let sd_cs = Output::new(p.PA4, Level::High, Speed::High);

    let sd_spi = BitbangSpiDevice::new(sd_sck, sd_mosi, sd_miso, Delay);
    let mut logger = SdCardLogger::new(sd_spi, sd_cs, Delay, StaticTimeSource::default());

    match logger.enable_rotation(1_000_000) {
        Ok(()) => defmt::info!("sd: rotation on, 1 MiB/file"),
        Err(e) => defmt::error!("sd rotation failed: {}", e),
    }
    logger.set_max_total_size(400_000_000);

    match logger.init() {
        Ok(()) => {
            defmt::info!(
                "sd: card size {} bytes",
                logger.card_size_bytes().unwrap_or(0)
            );
            defmt::info!("sd: card type {}", logger.card_type());
        }
        Err(e) => defmt::error!("sd init failed: {}", e),
    }

    // main loop

    loop {
        info!("receiving...");
        let mut buf = [0u8; 255];
        match radio.recv_bytes(&mut buf).await {
            Ok(recv) => {
                defmt::info!("-> {}", recv);
                let mut frame = [0u8; 256];
                frame[0] = recv.len() as u8;
                frame[1..1 + recv.len()].copy_from_slice(recv);

                match logger.append(&frame[..1 + recv.len()]) {
                    Ok(()) => defmt::info!("sd: logged {} bytes", recv.len()),
                    Err(e) => defmt::error!("sd write failed: {}", e),
                }
            }
            Err(_) => defmt::error!("receive failed"),
        }
    }
}

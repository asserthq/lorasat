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
use sat_drivers::sd::SdCardLogger;

const LORA_FREQ_IN_HZ: u32 = 435_100_000;

bind_interrupts!(struct Irqs {
    // SPI1 => spi::InterruptHandler<peripheral::SPI1>;
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>; // SPI TX
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>; // SPI RX
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;
});

// ── bitbang SPI (blocking) для SD-карты ──
// embassy-stm32 SPI — async. embedded-sdmmc хочет blocking SpiDevice.
// CS управляет сам embedded-sdmmc через отдельный пин, поэтому тут только
// SCK/MOSI/MISO. Пин подставь под свою распайку.

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

            read = (read << 1) | if self.miso.is_high().unwrap_or(false) { 1 } else { 0 };

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

    let nss = Output::new(p.PB12, Level::High, Speed::Low);
    let reset = Output::new(p.PA8, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PA9, p.EXTI9, Pull::Up, Irqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
    );

    let spi_device = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    // init lora radio

    let mut radio = Radio::new(spi_device, reset, irq, Delay, LORA_FREQ_IN_HZ)
        .await
        .unwrap();

    // init SD card (SPI bitbang). TODO: подставь свои пины.
    let sd_sck = Output::new(p.PA5, Level::Low, Speed::High);
    let sd_mosi = Output::new(p.PA7, Level::Low, Speed::High);
    let sd_miso = Input::new(p.PA6, Pull::Up);
    let sd_cs = Output::new(p.PA4, Level::High, Speed::High);

    let sd_spi = BitbangSpiDevice::new(sd_sck, sd_mosi, sd_miso, Delay);
    let mut logger = SdCardLogger::new(sd_spi, sd_cs, Delay);

    // main loop

    loop {
        info!("receiving...");
        let mut buf = [0u8; 255];
        match radio.try_recv_bytes(&mut buf).await {
            Ok(recv) => {
                defmt::info!("-> {}", recv);
                match logger.append(recv) {
                    Ok(()) => defmt::info!("sd: logged {} bytes", recv.len()),
                    Err(e) => defmt::error!("sd write failed: {}", e),
                }
            }
            Err(_) => defmt::error!("receive failed"),
        }
    }
}

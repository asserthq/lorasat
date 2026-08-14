//! Code works on STM32 Blue Pill board.
//! MCU STM32F103C8T6
//! Radio module SX1278 433MHz (Ra-1).
//! It demonstrates LORA P2P send functionality
#![no_std]
#![no_main]

use beacon::*;

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::LoRa;
use lora_phy::iv::GenericSx127xInterfaceVariant;
use lora_phy::sx127x::{Sx127x, Sx1276};
use lora_phy::{mod_params::*, sx127x};
use {defmt_rtt as _, panic_probe as _};

const LORA_FREQUENCY_IN_HZ: u32 = 433_000_000;

bind_interrupts!(struct Irqs {
    // SPI1 => spi::InterruptHandler<peripheral::SPI1>;
    DMA1_CHANNEL2 => dma::InterruptHandler<peripherals::DMA1_CH2>;
    DMA1_CHANNEL3 => dma::InterruptHandler<peripherals::DMA1_CH3>;
    EXTI0 => exti::InterruptHandler<interrupt::typelevel::EXTI0>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    let nss = Output::new(p.PA4, Level::High, Speed::Low);
    let reset = Output::new(p.PB1, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PB0, p.EXTI0, Pull::Up, Irqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA1_CH3, p.DMA1_CH2, Irqs, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    let config = sx127x::Config {
        chip: Sx1276,
        tcxo_used: false,
        rx_boost: false,
        tx_boost: true,
    };
    let iv = GenericSx127xInterfaceVariant::new(reset, irq, None, None).unwrap();
    let mut lora = LoRa::new(Sx127x::new(spi, iv, config), false, Delay)
        .await
        .unwrap();

    spawner.spawn(beacon_task(lora, Duration::from_secs(10)));
}

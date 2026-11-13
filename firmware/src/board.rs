use embassy_stm32::{
    Config, Peripherals, bind_interrupts, dma,
    exti::{self, ExtiInput},
    gpio::{Level, Output, Pull, Speed},
    interrupt, peripherals,
    rcc::Sysclk,
    spi::{self, Spi},
    time::khz,
};
use embassy_stm32::{mode::Async, spi::mode::Master};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use crate::config;

bind_interrupts!(pub(crate) struct Irqs {
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>;
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>;
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>;
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>;

    EXTI4 => exti::InterruptHandler<interrupt::typelevel::EXTI4>;
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;
    EXTI15_10 => exti::InterruptHandler<interrupt::typelevel::EXTI15_10>;
});

pub(crate) type RadioSpiBus = Spi<'static, Async, Master>;
pub(crate) type RadioSpiDevice = ExclusiveDevice<RadioSpiBus, Output<'static>, Delay>;

/// HAL-level resources required to construct one SX1262 driver.
///
/// Concrete STM32 peripheral and pin types are erased at the BSP boundary.
pub(crate) struct RadioParts {
    pub spi: RadioSpiDevice,
    pub reset: Output<'static>,
    pub dio1: ExtiInput<'static, Async>,
    pub busy: ExtiInput<'static, Async>,
}

pub(crate) struct BoardParts {
    pub platform_radio: RadioParts,
    pub service_radio: RadioParts,
}

pub(crate) fn init() -> BoardParts {
    let mut config = Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = Sysclk::HSI;

    split(embassy_stm32::init(config))
}

/// Applies this board's pin mapping and splits MCU peripherals by device.
pub(crate) fn split(p: Peripherals) -> BoardParts {
    let platform_radio = {
        let nss = Output::new(p.PA4, Level::High, Speed::Low);
        let reset = Output::new(p.PB6, Level::High, Speed::Low);
        let dio1 = ExtiInput::new(p.PB4, p.EXTI4, Pull::Up, Irqs);
        let busy = ExtiInput::new(p.PB5, p.EXTI5, Pull::Up, Irqs);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(config::radio::SPI_FREQUENCY_KHZ);
        let spi = Spi::new(
            p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH2, Irqs, spi_config,
        );
        let spi = ExclusiveDevice::new(spi, nss, Delay).expect("GPIO chip-select is infallible");

        RadioParts {
            spi,
            reset,
            dio1,
            busy,
        }
    };

    let service_radio = {
        // TODO(board): replace this temporary pinout after checking the schematic.
        let nss = Output::new(p.PB12, Level::High, Speed::Low);
        let reset = Output::new(p.PA8, Level::High, Speed::Low);
        let dio1 = ExtiInput::new(p.PA9, p.EXTI9, Pull::Up, Irqs);
        let busy = ExtiInput::new(p.PA10, p.EXTI10, Pull::Up, Irqs);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(config::radio::SPI_FREQUENCY_KHZ);
        let spi = Spi::new(
            p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
        );
        let spi = ExclusiveDevice::new(spi, nss, Delay).expect("GPIO chip-select is infallible");

        RadioParts {
            spi,
            reset,
            dio1,
            busy,
        }
    };

    BoardParts {
        platform_radio,
        service_radio,
    }
}

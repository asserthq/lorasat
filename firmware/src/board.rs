use embassy_stm32::mode::{Async, Blocking};
use embassy_stm32::time::Hertz;
use embassy_stm32::{
    Config, Peripherals, bind_interrupts, dma,
    exti::{self, ExtiInput},
    gpio::{Level, Output, Pull, Speed},
    i2c::{self, I2c},
    interrupt, peripherals,
    rcc::Sysclk,
    spi::{self, Spi},
    time::khz,
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use crate::config;

bind_interrupts!(pub(crate) struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;

    I2C2_EV => i2c::EventInterruptHandler<peripherals::I2C2>;
    I2C2_ER => i2c::ErrorInterruptHandler<peripherals::I2C2>;

    I2C3_EV => i2c::EventInterruptHandler<peripherals::I2C3>;
    I2C3_ER => i2c::ErrorInterruptHandler<peripherals::I2C3>;

    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>;
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>;

    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
    DMA1_STREAM1 => dma::InterruptHandler<peripherals::DMA1_CH1>;
    DMA1_STREAM2 => dma::InterruptHandler<peripherals::DMA1_CH2>;
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>;
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>;
    DMA1_STREAM5 => dma::InterruptHandler<peripherals::DMA1_CH5>;
    DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>;
    DMA1_STREAM7 => dma::InterruptHandler<peripherals::DMA1_CH7>;

    EXTI4 => exti::InterruptHandler<interrupt::typelevel::EXTI4>;
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;
    EXTI15_10 => exti::InterruptHandler<interrupt::typelevel::EXTI15_10>;
});

pub(crate) type I2cBus = I2c<'static, Async, i2c::mode::Master>;

pub(crate) type RadioSpiBus = Spi<'static, Async, spi::mode::Master>;
pub(crate) type RadioSpiDevice = ExclusiveDevice<RadioSpiBus, Output<'static>, Delay>;

pub(crate) type FlashSpiBus = Spi<'static, Blocking, spi::mode::Master>;
pub(crate) type FlashSpiDevice = ExclusiveDevice<FlashSpiBus, Output<'static>, Delay>;

pub(crate) struct SensorsParts {
    pub i2c1: I2cBus,
    pub i2c2: I2cBus,
    pub i2c3: I2cBus,
}

/// HAL-level resources required to construct one SX1262 driver.
///
/// Concrete STM32 peripheral and pin types are erased at the BSP boundary.
pub(crate) struct RadioParts {
    pub spi: RadioSpiDevice,
    pub reset: Output<'static>,
    pub dio1: ExtiInput<'static, Async>,
    pub busy: ExtiInput<'static, Async>,
}

pub(crate) struct FlashParts {
    pub spi: FlashSpiDevice,
}

pub(crate) struct BoardParts {
    pub platform_radio: RadioParts,
    pub service_radio: RadioParts,
    pub flash: FlashParts,
    pub sensors: SensorsParts,
}

pub(crate) fn init() -> BoardParts {
    let mut config = Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = Sysclk::HSI;

    split(embassy_stm32::init(config))
}

/// Applies this board's pin mapping and splits MCU peripherals by device.
pub(crate) fn split(p: Peripherals) -> BoardParts {
    let sensors = {
        let mut config = i2c::Config::default();
        config.frequency = Hertz(400_000);
        config.sda_pullup = false;
        config.scl_pullup = false;

        let i2c1 = I2c::new(p.I2C1, p.PB8, p.PB9, p.DMA1_CH6, p.DMA1_CH0, Irqs, config);
        let i2c2 = I2c::new(p.I2C2, p.PB10, p.PB3, p.DMA1_CH7, p.DMA1_CH2, Irqs, config);
        let i2c3 = I2c::new(p.I2C3, p.PA8, p.PC9, p.DMA1_CH5, p.DMA1_CH1, Irqs, config);

        SensorsParts { i2c1, i2c2, i2c3 }
    };

    let platform_radio = {
        let nss = Output::new(p.PA4, Level::High, Speed::Low);
        let reset = Output::new(p.PB6, Level::High, Speed::Low);
        let dio1 = ExtiInput::new(p.PB4, p.EXTI4, Pull::Up, Irqs);
        let busy = ExtiInput::new(p.PB5, p.EXTI5, Pull::Up, Irqs);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(config::radio::SPI_FREQUENCY_KHZ);
        let spi1 = Spi::new(
            p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH2, Irqs, spi_config,
        );
        let spi1 = ExclusiveDevice::new(spi1, nss, Delay).expect("GPIO chip-select is infallible");

        RadioParts {
            spi: spi1,
            reset,
            dio1,
            busy,
        }
    };

    let service_radio = {
        // TODO(board): replace this temporary pinout after checking the schematic.
        let nss = Output::new(p.PB12, Level::High, Speed::Low);
        // PA8 is the only available I2C3 SCL pin on this MCU package.
        let reset = Output::new(p.PC8, Level::High, Speed::Low);
        let dio1 = ExtiInput::new(p.PA9, p.EXTI9, Pull::Up, Irqs);
        let busy = ExtiInput::new(p.PA10, p.EXTI10, Pull::Up, Irqs);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(config::radio::SPI_FREQUENCY_KHZ);
        let spi2 = Spi::new(
            p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
        );
        let spi2 = ExclusiveDevice::new(spi2, nss, Delay).expect("GPIO chip-select is infallible");

        RadioParts {
            spi: spi2,
            reset,
            dio1,
            busy,
        }
    };

    let flash = {
        // TODO(board): verify the SPI3 and chip-select pins against the schematic.
        // DMA1 is fully occupied by SPI2 and the three asynchronous I2C buses,
        // so the dedicated flash bus intentionally uses blocking SPI transactions.
        let nss = Output::new(p.PA15, Level::High, Speed::VeryHigh);
        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(config::flash::SPI_FREQUENCY_KHZ);
        let spi3 = Spi::new_blocking(p.SPI3, p.PC10, p.PC12, p.PC11, spi_config);
        let spi3 = ExclusiveDevice::new(spi3, nss, Delay).expect("GPIO chip-select is infallible");

        FlashParts { spi: spi3 }
    };

    BoardParts {
        platform_radio,
        service_radio,
        flash,
        sensors,
    }
}

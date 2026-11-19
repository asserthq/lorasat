use embassy_stm32::{
    Peri, bind_interrupts, dma,
    exti::{self, ExtiInput},
    gpio::{Level, Output, Pull, Speed},
    interrupt,
    mode::Async,
    peripherals,
    spi::{self, Spi},
    time::khz,
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use crate::config;

bind_interrupts!(pub(crate) struct RadioIrqs {
    // Platform radio: SPI2.
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>;
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>;

    // Service radio: SPI3.
    DMA1_STREAM2 => dma::InterruptHandler<peripherals::DMA1_CH2>;
    DMA1_STREAM5 => dma::InterruptHandler<peripherals::DMA1_CH5>;

    EXTI1 => exti::InterruptHandler<interrupt::typelevel::EXTI1>;
    EXTI2 => exti::InterruptHandler<interrupt::typelevel::EXTI2>;
    EXTI4 => exti::InterruptHandler<interrupt::typelevel::EXTI4>;
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>;
});

pub(crate) type RadioSpiBus = Spi<'static, Async, spi::mode::Master>;
pub(crate) type RadioSpiDevice = ExclusiveDevice<RadioSpiBus, Output<'static>, Delay>;

pub(crate) struct Sx1262Parts {
    pub spi: RadioSpiDevice,
    pub reset: Output<'static>,
    pub dio1: ExtiInput<'static, Async>,
    pub busy: ExtiInput<'static, Async>,
}

pub(crate) struct RadioParts {
    pub platform: Sx1262Parts,
    pub service: Sx1262Parts,
}

pub(crate) struct PlatformRadioHardware {
    pub spi: Peri<'static, peripherals::SPI2>,
    pub sck: Peri<'static, peripherals::PB13>,
    pub mosi: Peri<'static, peripherals::PB15>,
    pub miso: Peri<'static, peripherals::PB14>,

    pub tx_dma: Peri<'static, peripherals::DMA1_CH4>,
    pub rx_dma: Peri<'static, peripherals::DMA1_CH3>,
    pub nss: Peri<'static, peripherals::PB12>,
    pub reset: Peri<'static, peripherals::PD3>,
    pub dio1: Peri<'static, peripherals::PD4>,
    pub dio1_exti: Peri<'static, peripherals::EXTI4>,
    pub busy: Peri<'static, peripherals::PD5>,
    pub busy_exti: Peri<'static, peripherals::EXTI5>,
}

pub(crate) struct ServiceRadioHardware {
    pub spi: Peri<'static, peripherals::SPI3>,
    pub sck: Peri<'static, peripherals::PC10>,
    pub mosi: Peri<'static, peripherals::PC12>,
    pub miso: Peri<'static, peripherals::PC11>,

    pub tx_dma: Peri<'static, peripherals::DMA1_CH5>,
    pub rx_dma: Peri<'static, peripherals::DMA1_CH2>,
    pub nss: Peri<'static, peripherals::PA15>,
    pub reset: Peri<'static, peripherals::PD0>,
    pub dio1: Peri<'static, peripherals::PD2>,
    pub dio1_exti: Peri<'static, peripherals::EXTI2>,
    pub busy: Peri<'static, peripherals::PD1>,
    pub busy_exti: Peri<'static, peripherals::EXTI1>,
}

pub(crate) fn init(platform: PlatformRadioHardware, service: ServiceRadioHardware) -> RadioParts {
    RadioParts {
        platform: init_platform(platform),
        service: init_service(service),
    }
}

fn init_platform(hw: PlatformRadioHardware) -> Sx1262Parts {
    let nss = Output::new(hw.nss, Level::High, Speed::Low);
    let reset = Output::new(hw.reset, Level::High, Speed::Low);
    let dio1 = ExtiInput::new(hw.dio1, hw.dio1_exti, Pull::Up, RadioIrqs);
    let busy = ExtiInput::new(hw.busy, hw.busy_exti, Pull::Up, RadioIrqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(config::radio::SPI_FREQUENCY_KHZ);
    let spi = Spi::new(
        hw.spi, hw.sck, hw.mosi, hw.miso, hw.tx_dma, hw.rx_dma, RadioIrqs, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).expect("GPIO chip-select is infallible");

    Sx1262Parts {
        spi,
        reset,
        dio1,
        busy,
    }
}

fn init_service(hw: ServiceRadioHardware) -> Sx1262Parts {
    let nss = Output::new(hw.nss, Level::High, Speed::Low);
    let reset = Output::new(hw.reset, Level::High, Speed::Low);
    let dio1 = ExtiInput::new(hw.dio1, hw.dio1_exti, Pull::Up, RadioIrqs);
    let busy = ExtiInput::new(hw.busy, hw.busy_exti, Pull::Up, RadioIrqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(config::radio::SPI_FREQUENCY_KHZ);
    let spi = Spi::new(
        hw.spi, hw.sck, hw.mosi, hw.miso, hw.tx_dma, hw.rx_dma, RadioIrqs, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).expect("GPIO chip-select is infallible");

    Sx1262Parts {
        spi,
        reset,
        dio1,
        busy,
    }
}

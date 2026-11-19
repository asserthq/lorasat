use embassy_stm32::{
    Peri, bind_interrupts, dma,
    gpio::{Level, Output, Speed},
    mode::Async,
    peripherals,
    spi::{self, Spi},
    time::khz,
};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use crate::config;

bind_interrupts!(pub(crate) struct FlashIrqs {
    DMA2_STREAM0 => dma::InterruptHandler<peripherals::DMA2_CH0>;
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>;
});

pub(crate) type FlashSpiBus = Spi<'static, Async, spi::mode::Master>;
pub(crate) type FlashSpiDevice = ExclusiveDevice<FlashSpiBus, Output<'static>, Delay>;

pub(crate) struct FlashParts {
    pub spi: FlashSpiDevice,
}

pub(crate) struct FlashHardware {
    pub spi: Peri<'static, peripherals::SPI1>,

    pub sck: Peri<'static, peripherals::PA5>,
    pub mosi: Peri<'static, peripherals::PA7>,
    pub miso: Peri<'static, peripherals::PA6>,
    pub tx_dma: Peri<'static, peripherals::DMA2_CH3>,
    pub rx_dma: Peri<'static, peripherals::DMA2_CH0>,

    pub nss: Peri<'static, peripherals::PA4>,
}

pub(crate) fn init(hw: FlashHardware) -> FlashParts {
    let nss = Output::new(hw.nss, Level::High, Speed::VeryHigh);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(config::flash::SPI_FREQUENCY_KHZ);
    let spi = Spi::new(
        hw.spi, hw.sck, hw.mosi, hw.miso, hw.tx_dma, hw.rx_dma, FlashIrqs, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).expect("GPIO chip-select is infallible");

    FlashParts { spi }
}

#![no_std]
#![no_main]

use defmt::{error, info};

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::time::mhz;
use embassy_stm32::{bind_interrupts, dma, peripherals, spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use sat_drivers::flash::{CAPACITY, SECTOR_SIZE, W25Q64};

use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>; // SPI3 RX
    DMA1_STREAM5 => dma::InterruptHandler<peripherals::DMA1_CH5>; // SPI3 TX
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    info!("flash erase start");

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

    let mut count = 0u32;
    for addr in (0..CAPACITY).step_by(SECTOR_SIZE as usize) {
        if let Err(e) = flash.erase_sector(addr) {
            error!("flash: erase sector 0x{:06x} failed: {:?}", addr, e);
            return;
        }
        count += 1;
    }
    info!("flash: erased {} sectors", count);
}

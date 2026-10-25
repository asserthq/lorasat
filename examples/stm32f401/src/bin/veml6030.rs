#![no_std]
#![no_main]

use defmt::info;

use embassy_executor::Spawner;
use embassy_stm32::i2c::{self, I2c};
use embassy_stm32::{bind_interrupts, dma, peripherals};
use embassy_time::Delay;
use embedded_hal_async::delay::DelayNs;

use veml6030::{SlaveAddr, Veml6030};

use sandbox_lib as _;

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>; // I2C1 TX
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>; // I2C1 RX
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Default::default());

    let mut config = i2c::Config::default();
    config.sda_pullup = true;
    config.scl_pullup = true;

    let i2c = I2c::new(
        p.I2C1, p.PB6, // SCL
        p.PB7, // SDA
        p.DMA1_CH6, p.DMA1_CH0, Irqs, config,
    );

    let dev = SlaveAddr::default();
    let mut sensor = Veml6030::new(i2c, dev);
    sensor.enable().unwrap();
    loop {
        let lux = sensor.read_lux().unwrap();
        info!("lux: {}", lux);
    }
}

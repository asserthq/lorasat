#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;

use embassy_stm32::{bind_interrupts, dma, i2c, peripherals};
use embassy_time::{Delay, Timer};
use mmc5983_rs::Mmc5983;
use sandbox_lib as _;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>; // rx dma
    DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>; // tx dma
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("STM32 initialized");

    let i2c_config = i2c::Config::default();

    let i2c = i2c::I2c::new(
        p.I2C1, p.PB8, p.PB9, p.DMA1_CH6, p.DMA1_CH0, Irqs, i2c_config,
    );
    info!("I2C1 initialized");

    let mut sensor = Mmc5983::new_with_i2c(i2c);
    info!("MMC5983MA created");

    sensor.init().await.unwrap();
    info!("MMC5983MA initialized");

    let mut delay = Delay;

    let _offset = sensor.calibrate_offset(&mut delay).await.unwrap();
    info!("Calibrated offset");

    loop {
        match sensor.magnetic_field().await {
            Ok(data) => {
                info!(
                    "Magnetic field: x {} y {} z {}",
                    data.x_gauss(),
                    data.y_gauss(),
                    data.z_gauss()
                );
            }
            Err(_) => {
                error!("MMC5983MA measurement error");
            }
        }
        Timer::after_millis(200).await;
    }
}

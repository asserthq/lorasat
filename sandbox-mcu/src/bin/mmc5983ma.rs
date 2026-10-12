#![no_std]
#![no_main]

use defmt::info;

use embassy_executor::Spawner;
use embassy_stm32::i2c::{self, I2c};
use embassy_stm32::{bind_interrupts, dma, peripherals};
use embassy_time::Delay;
use embedded_hal_async::delay::DelayNs;

use sandbox_lib as _;
use sat_drivers::magnetometer::Mmc5983Ma;

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
        p.I2C1,
        p.PB8, // SCL
        p.PB9, // SDA
        p.DMA1_CH6,
        p.DMA1_CH0,
        Irqs,
        config,
    );

    let mut mag = Mmc5983Ma::new(i2c, Delay).await.unwrap();

    let offset = mag.calibrate_offset().await.unwrap();
    info!(
        "Intrinsic offset: X={} Y={} Z={} Gauss",
        offset.x_gauss(),
        offset.y_gauss(),
        offset.z_gauss()
    );

    loop {
        match mag.magnetic_field().await {
            Ok(field) => info!(
                "Magnetic field: X={} Y={} Z={} Gauss",
                field.x_gauss(),
                field.y_gauss(),
                field.z_gauss()
            ),
            Err(_) => info!("magnetic field read failed"),
        }

        match mag.temperature().await {
            Ok(temp) => info!("Temperature: {} °C", temp.degrees_celsius()),
            Err(_) => info!("temperature read failed"),
        }

        Delay.delay_ms(5000).await;
    }
}

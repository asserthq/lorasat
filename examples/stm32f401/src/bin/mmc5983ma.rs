#![no_std]
#![no_main]

use defmt::{Debug2Format, info, warn};
use embassy_executor::Spawner;
use embassy_stm32::i2c::{self, I2c};
use embassy_stm32::time::Hertz;
use embassy_stm32::{bind_interrupts, dma, peripherals};
use embassy_time::{Delay, Duration, Timer, with_timeout};
use mmc5983_rs::Mmc5983;

use stm32f401_lib as _;

bind_interrupts!(struct Irqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;
    DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>;
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Default::default());
    let mut config = i2c::Config::default();
    config.frequency = Hertz(100_000);
    config.sda_pullup = true;
    config.scl_pullup = true;

    let i2c = I2c::new(
        p.I2C1, p.PB6, // SCL
        p.PB7, // SDA
        p.DMA1_CH6, p.DMA1_CH0, Irqs, config,
    );
    let mut delay = Delay;
    let mut mag = Mmc5983::new_with_i2c(i2c);
    Timer::after_millis(100).await;

    loop {
        match mag.product_id().await {
            Ok(id) if id.is_correct() => info!("MMC5983MA ID: {=u8:#04x}", id.raw()),
            Ok(id) => {
                warn!("Unexpected MMC5983MA ID: {=u8:#04x}", id.raw());
                Timer::after_secs(2).await;
                continue;
            }
            Err(error) => {
                warn!("MMC5983MA ID read failed: {:?}", Debug2Format(&error));
                Timer::after_secs(2).await;
                continue;
            }
        }

        // The local crate patch adds delays INSIDE reset/OTP initialization.
        match with_timeout(Duration::from_secs(1), mag.init_with_delay(&mut delay)).await {
            Ok(Ok(())) => info!("MMC5983MA ready (one-shot, 100 Hz bandwidth)"),
            Ok(Err(error)) => {
                warn!(
                    "MMC5983MA initialization failed: {:?}",
                    Debug2Format(&error)
                );
                Timer::after_secs(2).await;
                continue;
            }
            Err(_) => {
                warn!("MMC5983MA initialization timed out");
                Timer::after_secs(2).await;
                continue;
            }
        }

        loop {
            // Unlike read_magnetic_field(), this starts and awaits a NEW measurement.
            match with_timeout(Duration::from_millis(100), mag.magnetic_field()).await {
                Ok(Ok(field)) => info!(
                    "mag [gauss]: x={} y={} z={}",
                    field.x_gauss(),
                    field.y_gauss(),
                    field.z_gauss()
                ),
                Ok(Err(error)) => {
                    warn!("MMC5983MA read failed: {:?}", Debug2Format(&error));
                    Timer::after_secs(1).await;
                    break;
                }
                Err(_) => {
                    warn!("MMC5983MA measurement timed out");
                    Timer::after_secs(1).await;
                    break;
                }
            }
            Timer::after_millis(100).await;
        }
    }
}

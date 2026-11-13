#![no_std]
#![no_main]

use bmi088_driver::{
    AccBandwidthVariant, AccDataRateVariant, Bmi088Accelerometer, Bmi088Gyroscope,
    GyroBandwidthVariant, GyroPowerMode,
};
use defmt::{info, warn};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_stm32::i2c::{self, I2c};
use embassy_stm32::{bind_interrupts, dma, peripherals};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::{Delay, Timer};

use stm32f401_lib as _;

// SDO1/SDO2 low. Use 0x19/0x69 respectively when tied high.
const ACC_ADDRESS: u8 = 0x18;
const GYRO_ADDRESS: u8 = 0x68;

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
    config.sda_pullup = true;
    config.scl_pullup = true;

    let i2c = I2c::new(
        p.I2C1, p.PB6, // SCL
        p.PB7, // SDA
        p.DMA1_CH6, p.DMA1_CH0, Irqs, config,
    );
    let bus = Mutex::<NoopRawMutex, _>::new(i2c);
    let mut delay = Delay;

    // Let both BMI088 dies finish power-on before the first transaction.
    Timer::after_millis(50).await;

    let mut accelerometer =
        Bmi088Accelerometer::initialize_i2c(I2cDevice::new(&bus), ACC_ADDRESS, &mut delay)
            .await
            .expect("BMI088 accelerometer initialization failed");

    let mut gyroscope =
        Bmi088Gyroscope::initialize_i2c(I2cDevice::new(&bus), GYRO_ADDRESS, &mut delay)
            .await
            .expect("BMI088 gyroscope initialization failed");

    accelerometer
        .set_acc_bwp(AccBandwidthVariant::Normal)
        .await
        .expect("BMI088 accelerometer bandwidth configuration failed");
    accelerometer
        .set_acc_odr(AccDataRateVariant::Odr100)
        .await
        .expect("BMI088 accelerometer ODR configuration failed");

    gyroscope
        .set_power_mode(GyroPowerMode::Normal)
        .await
        .expect("BMI088 gyroscope power configuration failed");
    gyroscope
        .set_bandwidth(GyroBandwidthVariant::Odr100Bw32Hz)
        .await
        .expect("BMI088 gyroscope bandwidth configuration failed");

    // Driver defaults: accelerometer +/-24 g, gyroscope +/-2000 deg/s.
    Timer::after_millis(50).await;
    info!("BMI088 ready: 100 Hz, acc +/-24 g, gyro +/-2000 deg/s");

    loop {
        match accelerometer.read_acc_scaled().await {
            Ok([x, y, z]) => info!("acc [g]: x={} y={} z={}", x, y, z),
            Err(error) => warn!("BMI088 accelerometer read failed: {:?}", error),
        }

        match gyroscope.read_gyro_scaled().await {
            Ok([x, y, z]) => info!("gyro [deg/s]: x={} y={} z={}", x, y, z),
            Err(error) => warn!("BMI088 gyroscope read failed: {:?}", error),
        }

        match accelerometer.read_acc_temp().await {
            Ok(temperature) => info!("temperature [C]: {}", temperature),
            Err(error) => warn!("BMI088 temperature read failed: {:?}", error),
        }

        Timer::after_millis(100).await;
    }
}

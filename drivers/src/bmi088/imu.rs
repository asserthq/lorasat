use bmi088_driver::{
    AccBandwidthVariant, AccDataRateVariant, Bmi088Accelerometer, Bmi088Gyroscope,
    GyroBandwidthVariant, GyroPowerMode, compat_embedded_hal::I2cAny,
};
use core::f32::consts::PI;
use embedded_hal_async::{delay::DelayNs, i2c::I2c};
use sat_core::adcs::{Imu, ImuSample};

use super::ImuError;

const ACC_ADDRESS: u8 = 0x18;
const GYRO_ADDRESS: u8 = 0x68;

const DEG_TO_RAD: f32 = PI / 180.0;
const G_TO_M_S2: f32 = 9.806_65;

pub struct ImuBmi088<AccI2c, GyroI2c> {
    accel: Bmi088Accelerometer<I2cAny<AccI2c>>,
    gyro: Bmi088Gyroscope<I2cAny<GyroI2c>>,
}

impl<AccI2c, GyroI2c> ImuBmi088<AccI2c, GyroI2c>
where
    AccI2c: I2c,
    GyroI2c: I2c,
{
    pub async fn new(
        acc_i2c: AccI2c,
        gyro_i2c: GyroI2c,
        delay: &mut impl DelayNs,
    ) -> Result<Self, ImuError> {
        let mut accel = Bmi088Accelerometer::initialize_i2c(acc_i2c, ACC_ADDRESS, delay)
            .await
            .map_err(|_| ImuError::AccelerometerInit)?;

        let mut gyro = Bmi088Gyroscope::initialize_i2c(gyro_i2c, GYRO_ADDRESS, delay)
            .await
            .map_err(|_| ImuError::GyroscopeInit)?;

        accel
            .set_acc_bwp(AccBandwidthVariant::Normal)
            .await
            .map_err(|_| ImuError::AccelerometerConfig)?;

        accel
            .set_acc_odr(AccDataRateVariant::Odr100)
            .await
            .map_err(|_| ImuError::AccelerometerConfig)?;

        gyro.set_power_mode(GyroPowerMode::Normal)
            .await
            .map_err(|_| ImuError::GyroscopeConfig)?;

        gyro.set_bandwidth(GyroBandwidthVariant::Odr100Bw32Hz)
            .await
            .map_err(|_| ImuError::GyroscopeConfig)?;

        Ok(Self { accel, gyro })
    }
}

impl<AccI2c, GyroI2c> Imu for ImuBmi088<AccI2c, GyroI2c>
where
    AccI2c: I2c,
    GyroI2c: I2c,
{
    type Error = ImuError;

    async fn read_sample(&mut self) -> Result<ImuSample, Self::Error> {
        let [gx, gy, gz] = self
            .gyro
            .read_gyro_scaled()
            .await
            .map_err(|_| ImuError::GyroscopeRead)?;

        let [ax, ay, az] = self
            .accel
            .read_acc_scaled()
            .await
            .map_err(|_| ImuError::AccelerometerRead)?;

        let temperature_c = self
            .accel
            .read_acc_temp()
            .await
            .map_err(|_| ImuError::TemperatureRead)?;

        Ok(ImuSample {
            angular_velocity_rad_s: [gx * DEG_TO_RAD, gy * DEG_TO_RAD, gz * DEG_TO_RAD],
            acceleration_m_s2: [ax * G_TO_M_S2, ay * G_TO_M_S2, az * G_TO_M_S2],
            temperature_c,
        })
    }
}

use embedded_hal_async::i2c::I2c;
use sat_core::adcs::SunSensor;
use veml6030::{SlaveAddr, Veml6030};

use crate::veml6030::error::SunSensorError;

const MAX_LUX: f32 = 100_000.0;

pub struct SunSensorImpl<I2C: I2c> {
    sensor: Veml6030<I2C>,
}

impl<I2C: I2c> SunSensorImpl<I2C> {
    pub fn new(i2c: I2C) -> Self {
        Self {
            sensor: Veml6030::new(i2c, SlaveAddr::default()),
        }
    }

    pub async fn init(&mut self) -> Result<(), SunSensorError> {
        self.sensor
            .enable()
            .await
            .map_err(|_| SunSensorError::SunInitError)
    }
}

impl<I2C: I2c> SunSensor for SunSensorImpl<I2C> {
    type Error = SunSensorError;

    async fn read(&mut self) -> Result<f32, Self::Error> {
        let lux = self
            .sensor
            .read_lux()
            .await
            .map_err(|_| SunSensorError::ReadRawError)?;

        Ok((lux / MAX_LUX).clamp(0.0, 1.0))
    }
}

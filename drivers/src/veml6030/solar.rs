use embedded_hal_async::i2c::I2c;
use sat_core::adcs::SunSensor;
use veml6030::{SlaveAddr, Veml6030};

use crate::veml6030::error::SunSensorError;

pub struct SunSensorImpl<I2C> {
    sensor: Veml6030<I2C>,
}

impl<I2C> SunSensorImpl<I2C>
where
    I2C: I2c,
{
    pub fn new(i2c: I2C) -> Self {
        let sensor = Veml6030::new(i2c, SlaveAddr::default());

        Self { sensor }
    }
}

impl SunSensor for SunSensorImpl {
    type Error = SunSensorError;

    async fn read_raw(&mut self) -> Result<f32, Self::Error> {
        todo!()
    }
}

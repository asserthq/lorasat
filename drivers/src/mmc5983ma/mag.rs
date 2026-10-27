use embassy_stm32::i2c::I2c;
use mmc5983_rs::Mmc5983;
use nalgebra::Vector3;

use crate::mmc5983ma::error::MagError;

pub struct MagMmc5983<I2C> {
    sensor: Mmc5983<mmc5983_rs::interface::I2cInterface<I2C>, mmc5983_rs::mode::OneShot>,
}

impl<I2C> MagMmc5983<I2C>
where
    I2C: embedded_hal_async::i2c::I2c,
{
    pub async fn new(i2c: I2C) -> Result<Self, MagError> {
        let mut sensor = Mmc5983::new_with_i2c(i2c);

        sensor.init().await.map_err(|_| MagError::MagInitError)?;

        Ok(Self { sensor })
    }

    pub async fn read(&mut self) -> Result<Vector3<f32>, MagError> {
        let field = self
            .sensor
            .magnetic_field()
            .await
            .map_err(|_| MagError::MagReadError)?;

        let (x, y, z) = field.gauss();

        Ok(Vector3::new(x, y, z))
    }
}

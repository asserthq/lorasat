use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;
use mmc5983_rs::Mmc5983;
use mmc5983_rs::interface::I2cInterface;
use mmc5983_rs::mode::OneShot;
use nalgebra::Vector3;
use sat_core::adcs::Mag;

use crate::mmc5983ma::error::MagError;

pub struct MagMmc5983<I2C> {
    sensor: Mmc5983<I2cInterface<I2C>, OneShot>,
}

impl<I2C> MagMmc5983<I2C>
where
    I2C: I2c,
{
    pub async fn new<D: DelayNs>(i2c: I2C, delay: &mut D) -> Result<Self, MagError> {
        let mut sensor = Mmc5983::new_with_i2c(i2c);

        sensor
            .init_with_delay(delay)
            .await
            .map_err(|_| MagError::MagInitError)?;

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

impl<I2C> Mag for MagMmc5983<I2C>
where
    I2C: I2c,
{
    type Error = MagError;

    async fn read(&mut self) -> Result<Vector3<f32>, Self::Error> {
        MagMmc5983::read(self).await
    }
}

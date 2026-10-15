use mmc5983_rs::Mmc5983;
use nalgebra::Vector3;

pub struct MagMmc5983 {
    sensor: Mmc5983,
}

impl<I2C> MagMmc5983 {
    pub async fn new(i2c: I2C) -> Self {
        let sensor = Mmc5983::new_with_i2c(i2c);
        sensor.init().await.unwrap();
        Self { sensor }
    }

    pub async fn read(&mut self) -> Vector3<f32> {
        let (x, y, z) = self.sensor.magnetic_field().await.unwrap();
        Vector3::new(x, y, z)
    }
}

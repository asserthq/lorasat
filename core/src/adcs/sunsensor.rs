#[allow(async_fn_in_trait)]

pub trait SunSensor {
    type Error;
    /// Считать исходное значение освещённости (от 0 до 1)
    async fn read_raw(&mut self) -> Result<f32, Self::Error>;
}

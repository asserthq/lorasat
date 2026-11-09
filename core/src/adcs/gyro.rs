use nalgebra::Vector3;

#[allow(async_fn_in_trait)]
pub trait Gyro {
    type Error;
    async fn read(&self) -> Result<Vector3<f32>, Self::Error>;
}

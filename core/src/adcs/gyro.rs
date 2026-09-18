use nalgebra::Vector3;

#[allow(async_fn_in_trait)]
pub trait Gyro {
    async fn read(&self) -> Vector3<f32>;
}

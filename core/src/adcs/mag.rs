use nalgebra::Vector3;

#[allow(async_fn_in_trait)]
pub trait Mag {
    async fn read(&self) -> Vector3<f32>;
}
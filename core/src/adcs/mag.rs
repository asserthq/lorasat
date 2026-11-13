use core::fmt::Debug;
use nalgebra::Vector3;

#[allow(async_fn_in_trait)]
pub trait Mag {
    type Error: Debug;
    async fn read(&mut self) -> Result<Vector3<f32>, Self::Error>;
}

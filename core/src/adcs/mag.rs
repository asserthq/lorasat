use nalgebra::Vector3;

pub type BVec = Vector3<f32>;

#[allow(async_fn_in_trait)]
pub trait Mag {
    async fn read(&self) -> BVec;
}

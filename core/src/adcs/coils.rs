use nalgebra::Vector3;

#[allow(async_fn_in_trait)]
pub trait Coils {
    type Error;
    async fn apply_levels(&mut self, levels: Vector3<f32>) -> Result<(), Self::Error>;
}

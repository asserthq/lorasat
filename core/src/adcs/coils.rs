use nalgebra::Vector3;

#[allow(async_fn_in_trait)]
pub trait Coils {
    type Error;
    async fn apply_pwm(&mut self, levels: Vector3<f32>) -> Result<(), Self::Error>;
    async fn get_current(&mut self) -> Result<Vector3<f32>, Self::Error>;
}

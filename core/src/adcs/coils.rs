use nalgebra::Vector3;

pub type CoilLevelsVec = Vector3<f32>;

#[allow(async_fn_in_trait)]
pub trait Coils {
    async fn apply_levels(&mut self, levels: CoilLevelsVec);
}

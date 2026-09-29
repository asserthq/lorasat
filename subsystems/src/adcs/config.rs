use nalgebra::Matrix3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdcsConfig {
    pub mag_calib: Matrix3<f32>,
    pub gyro_calib: Matrix3<f32>,
    pub freq: f32,
}

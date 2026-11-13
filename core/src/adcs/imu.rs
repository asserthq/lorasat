#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImuSample {
    pub angular_velocity_rad_s: [f32; 3],
    pub acceleration_m_s2: [f32; 3],
    pub temperature_c: f32,
}

#[allow(async_fn_in_trait)]
pub trait Imu {
    type Error;
    async fn read_sample(&mut self) -> Result<ImuSample, Self::Error>;
}

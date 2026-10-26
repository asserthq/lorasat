#[allow(async_fn_in_trait)]
pub trait SunSensor {
    type Error;

    /// Возвращает нормализованную освещённость в диапазоне 0.0..=1.0.
    /// 0.0 — минимальный сигнал, 1.0 — максимальный сигнал
    /// в принятом диапазоне измерений датчика.
    async fn read(&mut self) -> Result<f32, Self::Error>;
}

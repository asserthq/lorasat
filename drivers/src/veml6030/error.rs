#[derive(Debug, PartialEq, defmt::Format)]
pub enum SunSensorError {
    SunInitError,
    ReadRawError,
    ReadLuxError,
}

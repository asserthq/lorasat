#[derive(Debug, PartialEq, defmt::Format)]
pub enum ImuError {
    AccelerometerInit,
    GyroscopeInit,
    AccelerometerConfig,
    GyroscopeConfig,
    AccelerometerRead,
    GyroscopeRead,
    TemperatureRead,
}

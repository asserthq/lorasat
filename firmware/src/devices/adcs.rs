use embassy_stm32::gpio::Output;
use sat_drivers::{
    bmi088::{ImuBmi088, ImuError},
    coils::{CoilsImpl, UnusedInput},
    mmc5983ma::{MagError, MagMmc5983},
    veml6030::{SunSensorError, SunSensorImpl},
};

use crate::{
    board::{CoilsParts, Tim3PwmChannel, Tim4PwmChannel},
    resources::shared_i2c::{SharedBlockingI2cDevice, SharedI2cBuses, SharedI2cDevice},
};

pub(crate) type Magnetometer = MagMmc5983<SharedI2cDevice>;
pub(crate) type Imu = ImuBmi088<SharedI2cDevice, SharedI2cDevice>;
pub(crate) type SunSensors = SunSensorImpl<SharedBlockingI2cDevice>;
pub(crate) type BoardCoils = CoilsImpl<
    Tim4PwmChannel,
    Tim4PwmChannel,
    Tim3PwmChannel,
    Tim3PwmChannel,
    Tim3PwmChannel,
    Tim3PwmChannel,
    Output<'static>,
    UnusedInput,
    UnusedInput,
>;

pub(crate) struct AdcsDevices {
    pub magnetometer: Magnetometer,
    pub imu: Imu,
    pub coils: BoardCoils,
    pub sun_sensors: SunSensors,
}

#[derive(Debug, defmt::Format)]
pub(crate) enum AdcsInitError {
    Magnetometer,
    Imu,
    SunSensor,
}

impl From<MagError> for AdcsInitError {
    fn from(_: MagError) -> Self {
        Self::Magnetometer
    }
}

impl From<ImuError> for AdcsInitError {
    fn from(_: ImuError) -> Self {
        Self::Imu
    }
}

impl From<SunSensorError> for AdcsInitError {
    fn from(_: SunSensorError) -> Self {
        Self::SunSensor
    }
}

pub(crate) async fn init(
    i2c: &SharedI2cBuses,
    coils: CoilsParts,
) -> Result<AdcsDevices, AdcsInitError> {
    let mut delay = embassy_time::Delay;

    // These devices share one physical bus. Each driver owns a separate
    // mutex-backed handle, so transactions remain serialized.
    let magnetometer = MagMmc5983::new(i2c.bus1(), &mut delay).await?;
    let imu = ImuBmi088::new(i2c.bus1(), i2c.bus1(), &mut delay).await?;

    // Illumination sensor is connected to another physical bus.
    let mut sun_sensors = SunSensorImpl::new(i2c.bus2());
    sun_sensors.init().await?;

    let CoilsParts {
        x_in1,
        x_in2,
        y_in1,
        y_in2,
        z_in1,
        z_in2,
        sleep,
    } = coils;

    let coils = CoilsImpl::new(
        x_in1,
        x_in2,
        y_in1,
        y_in2,
        z_in1,
        z_in2,
        Some(sleep),
        None::<UnusedInput>,
        None::<UnusedInput>,
    );

    Ok(AdcsDevices {
        magnetometer,
        imu,
        coils,
        sun_sensors,
    })
}

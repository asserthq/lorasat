use nalgebra::Vector3;
use sat_core::adcs::Coils;
use sat_drivers::mmc5983ma::{MagError, MagMmc5983};

use crate::board::I2cBus;

pub(crate) type Magnetometer = MagMmc5983<I2cBus>;

pub(crate) struct AdcsDevices {
    pub magnetometer: Magnetometer,
    pub coils: DisabledCoils,
}

pub(crate) async fn init(i2c: I2cBus) -> Result<AdcsDevices, MagError> {
    let mut delay = embassy_time::Delay;
    let magnetometer = MagMmc5983::new(i2c, &mut delay).await?;

    Ok(AdcsDevices {
        magnetometer,
        coils: DisabledCoils,
    })
}

/// Safe placeholder until the board BSP exposes the coil PWM and fault pins.
///
/// Idle mode can repeatedly apply zero output. Any non-zero request fails and
/// stops ADCS instead of pretending that actuator output was applied.
pub(crate) struct DisabledCoils;

#[derive(Debug, defmt::Format)]
pub(crate) struct CoilsUnavailable;

impl Coils for DisabledCoils {
    type Error = CoilsUnavailable;

    async fn apply_pwm(&mut self, levels: Vector3<f32>) -> Result<(), Self::Error> {
        if levels == Vector3::zeros() {
            Ok(())
        } else {
            Err(CoilsUnavailable)
        }
    }

    async fn get_current(&mut self) -> Result<Vector3<f32>, Self::Error> {
        Err(CoilsUnavailable)
    }
}

use drv8833_driver::{Breaks, MotorDriver, MotorDriverError, PwmMovement, PwmSplitDriverType};
use embedded_hal::{
    digital::{InputPin, OutputPin},
    pwm::SetDutyCycle,
};
use nalgebra::{SimdComplexField, Vector3};
use sat_core::adcs::Coils;

use crate::coils::error::CoilsError;

pub struct CoilsImpl<IN11, IN12, IN13, IN14, IN21, IN22, IN23, IN24, SLEEP1, FAULT1, SLEEP2, FAULT2>
where
    IN11: SetDutyCycle,
    IN12: SetDutyCycle,
    IN13: SetDutyCycle,
    IN14: SetDutyCycle,
    IN21: SetDutyCycle,
    IN22: SetDutyCycle,
    IN23: SetDutyCycle,
    IN24: SetDutyCycle,
    SLEEP1: OutputPin,
    FAULT1: InputPin,
    SLEEP2: OutputPin,
    FAULT2: InputPin,
{
    drv1: PwmSplitDriverType<IN11, IN12, IN13, IN14, SLEEP1, FAULT1>,
    drv2: PwmSplitDriverType<IN21, IN22, IN23, IN24, SLEEP2, FAULT2>,
}

impl<IN11, IN12, IN13, IN14, IN21, IN22, IN23, IN24, SLEEP1, FAULT1, SLEEP2, FAULT2>
    CoilsImpl<IN11, IN12, IN13, IN14, IN21, IN22, IN23, IN24, SLEEP1, FAULT1, SLEEP2, FAULT2>
where
    IN11: SetDutyCycle,
    IN12: SetDutyCycle,
    IN13: SetDutyCycle,
    IN14: SetDutyCycle,
    IN21: SetDutyCycle,
    IN22: SetDutyCycle,
    IN23: SetDutyCycle,
    IN24: SetDutyCycle,
    SLEEP1: OutputPin,
    FAULT1: InputPin,
    SLEEP2: OutputPin,
    FAULT2: InputPin,
{
    pub fn new(
        in11: IN11,
        in12: IN12,
        in13: IN13,
        in14: IN14,
        in21: IN21,
        in22: IN22,
        in23: IN23,
        in24: IN24,
        sleep1: Option<SLEEP1>,
        fault1: Option<FAULT1>,
        sleep2: Option<SLEEP2>,
        fault2: Option<FAULT2>,
    ) -> Self {
        let drv1 = MotorDriver::new_pwm_split(in11, in12, in13, in14, sleep1, fault1);
        let drv2 = MotorDriver::new_pwm_split(in21, in22, in23, in24, sleep2, fault2);
        Self { drv1, drv2 }
    }

    fn level_to_percent(level: f32) -> u8 {
        (level * 100.0).simd_round().abs() as u8
    }

    fn set_x(&mut self, level: f32) -> Result<(), MotorDriverError> {
        let per = Self::level_to_percent(level);
        if level == 0.0 {
            self.drv1.a.coast()
        } else if level > 0.0 {
            self.drv1.a.forward(per)
        } else {
            self.drv1.a.reverse(per)
        }
    }

    fn set_y(&mut self, level: f32) -> Result<(), MotorDriverError> {
        let per = Self::level_to_percent(level);
        if level == 0.0 {
            self.drv1.b.coast()
        } else if level > 0.0 {
            self.drv1.b.forward(per)
        } else {
            self.drv1.b.reverse(per)
        }
    }

    fn set_z(&mut self, level: f32) -> Result<(), MotorDriverError> {
        let per = Self::level_to_percent(level);
        if level == 0.0 {
            self.drv2.a.coast()
        } else if level > 0.0 {
            self.drv2.a.forward(per)
        } else {
            self.drv2.a.reverse(per)
        }
    }
}

impl<IN11, IN12, IN13, IN14, IN21, IN22, IN23, IN24, SLEEP1, FAULT1, SLEEP2, FAULT2> Coils
    for CoilsImpl<IN11, IN12, IN13, IN14, IN21, IN22, IN23, IN24, SLEEP1, FAULT1, SLEEP2, FAULT2>
where
    IN11: SetDutyCycle,
    IN12: SetDutyCycle,
    IN13: SetDutyCycle,
    IN14: SetDutyCycle,
    IN21: SetDutyCycle,
    IN22: SetDutyCycle,
    IN23: SetDutyCycle,
    IN24: SetDutyCycle,
    SLEEP1: OutputPin,
    FAULT1: InputPin,
    SLEEP2: OutputPin,
    FAULT2: InputPin,
{
    type Error = CoilsError;

    async fn apply_pwm(&mut self, levels: Vector3<f32>) -> Result<(), Self::Error> {
        if !levels.iter().all(|x| x.is_finite() && x.abs() <= 1.0) {
            Err(CoilsError::LevelOutOfRange)
        } else {
            self.set_x(levels.x)
                .map_err(|_| CoilsError::SetPwmOnCoil(0))?;
            self.set_y(levels.y)
                .map_err(|_| CoilsError::SetPwmOnCoil(1))?;
            self.set_z(levels.z)
                .map_err(|_| CoilsError::SetPwmOnCoil(2))?;

            Ok(())
        }
    }

    async fn get_current(&mut self) -> Result<Vector3<f32>, Self::Error> {
        todo!()
    }
}

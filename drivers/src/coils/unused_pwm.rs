use core::convert::Infallible;

use embedded_hal::pwm::{ErrorType, SetDutyCycle};

/// Placeholder for an unused PWM input; does not control any hardware.
pub struct UnusedPwm;

impl ErrorType for UnusedPwm {
    type Error = Infallible;
}

impl SetDutyCycle for UnusedPwm {
    fn max_duty_cycle(&self) -> u16 {
        u16::MAX
    }

    fn set_duty_cycle(&mut self, _duty: u16) -> Result<(), Self::Error> {
        Ok(())
    }
}

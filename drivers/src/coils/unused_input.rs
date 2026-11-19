use core::convert::Infallible;

use embedded_hal::digital::{ErrorType, InputPin};

/// Placeholder for a fault input that is not connected on the board.
pub struct UnusedInput;

impl ErrorType for UnusedInput {
    type Error = Infallible;
}

impl InputPin for UnusedInput {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(false)
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

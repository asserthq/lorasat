#[derive(Debug, PartialEq)]
#[derive(defmt::Format)]
pub enum AdcsError {
    CoilsPwmError,
}

#[derive(Debug, PartialEq, defmt::Format)]
pub enum AdcsError {
    CoilsPwmError,
}

use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScpError {
    BufferTooShort,
    InvalidControlByte,
}

impl fmt::Display for ScpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort => write!(f, "Buffer too short for SCP message"),
            Self::InvalidControlByte => write!(f, "Invalid SCP control byte"),
        }
    }
}

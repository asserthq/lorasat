use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolError {
    BufferTooShort,
    InvalidFrameType,
    InvalidProtocolVersion,
    DataTooLarge,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort => write!(f, "Buffer too short for parsing"),
            Self::InvalidFrameType => write!(f, "Invalid or unsupported frame type"),
            Self::InvalidProtocolVersion => write!(f, "Invalid protocol version (expected 0x00)"),
            Self::DataTooLarge => write!(f, "Data payload exceeds maximum size (238 bytes)"),
        }
    }
}

impl core::error::Error for ProtocolError {}

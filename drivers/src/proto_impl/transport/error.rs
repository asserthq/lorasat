use core::fmt;

#[derive(defmt::Format, Debug, PartialEq)]
pub enum TransportError<LE: Debug> {
    InvalidControlByte,
    BufferTooShort,
    InvalidSessionId,
    ChunkIndexOutOfBounds,
    SessionNotInitialized,
    Decode,
    Encode,
    Link(LE),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidControlByte => f.write_str("invalid FRP control byte"),
            Self::BufferTooShort => f.write_str("buffer too short for FRP message"),
            Self::InvalidSessionId => f.write_str("FRP session ID mismatch"),
            Self::ChunkIndexOutOfBounds => f.write_str("FRP chunk index out of bounds"),
            Self::SessionNotInitialized => f.write_str("FRP receiver session not initialized"),
            Self::Decode => f.write_str("FRP decode error"),
            Self::Encode => f.write_str("FRP encode error"),
        }
    }
}

impl From<postcard::Error> for FrpError {
    fn from(e: postcard::Error) -> Self {
        match e {
            postcard::Error::SerializeBufferFull => FrpError::BufferTooShort,
            _ => FrpError::Decode,
        }
    }
}

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq, defmt::Format)]
pub enum Error {
    EncodeError,
    DecodeError,
    TxError,
    RxError,
    LogicError,
    NotSupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EncodeError => f.write_str("encode error"),
            Error::DecodeError => f.write_str("decode error"),
            Error::NotSupported => f.write_str("feature not supported"),
            Error::TxError => f.write_str("tx error"),
            Error::RxError => f.write_str("rx error"),
            Error::LogicError => f.write_str("logic error"),
        }
    }
}

impl From<postcard::Error> for Error {
    fn from(e: postcard::Error) -> Self {
        match e {
            postcard::Error::SerializeBufferFull => Error::EncodeError,
            postcard::Error::SerdeSerCustom => Error::EncodeError,
            postcard::Error::CollectStrError => Error::EncodeError,

            postcard::Error::WontImplement => Error::EncodeError,
            postcard::Error::NotYetImplemented => Error::EncodeError,

            _ => Error::DecodeError,
        }
    }
}

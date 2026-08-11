use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq, defmt::Format)]
pub enum Error {
    TransportSend,
    TransportRecv,

    DataLinkEncode,
    DataLinkDecode,

    PhysicalTx,
    PhysicalRx,

    Logic,
    NotSupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TransportSend => f.write_str("transport send error"),
            Error::TransportRecv => f.write_str("transport receive error"),
            Error::DataLinkEncode => f.write_str("data link encode error"),
            Error::DataLinkDecode => f.write_str("data link decode error"),
            Error::PhysicalTx => f.write_str("physical tx error"),
            Error::PhysicalRx => f.write_str("physical rx error"),
            Error::Logic => f.write_str("logic error"),
            Error::NotSupported => f.write_str("feature not supported"),
        }
    }
}

impl From<postcard::Error> for Error {
    fn from(e: postcard::Error) -> Self {
        match e {
            postcard::Error::SerializeBufferFull => Error::DataLinkEncode,
            postcard::Error::SerdeSerCustom => Error::DataLinkEncode,
            postcard::Error::CollectStrError => Error::DataLinkEncode,

            postcard::Error::WontImplement => Error::DataLinkEncode,
            postcard::Error::NotYetImplemented => Error::DataLinkEncode,

            _ => Error::DataLinkDecode,
        }
    }
}

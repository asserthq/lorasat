use core::fmt::Debug;

use crate::error;
use crate::physical::PhysicalLayer;

#[derive(Debug)]
pub enum CodecError<P: PhysicalLayer + Debug> {
    Encode,
    Decode,
    Physical(P::Error),
}

impl<P: PhysicalLayer + Debug> From<postcard::Error> for CodecError<P> {
    fn from(e: postcard::Error) -> Self {
        match e {
            postcard::Error::SerializeBufferFull
            | postcard::Error::SerdeSerCustom
            | postcard::Error::CollectStrError
            | postcard::Error::WontImplement
            | postcard::Error::NotYetImplemented => CodecError::Encode,

            _ => CodecError::Decode,
        }
    }
}

impl<P: PhysicalLayer + Debug> From<error::Error> for CodecError<P> {
    fn from(e: error::Error) -> Self {
        match e {
            error::Error::DataLinkEncode => CodecError::Encode,
            error::Error::DataLinkDecode => CodecError::Decode,
            _ => CodecError::Decode,
        }
    }
}

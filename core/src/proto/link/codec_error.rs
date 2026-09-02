use core::fmt::Debug;

use crate::error;
use crate::layer::phy::PhyLayer;

#[derive(Debug, PartialEq)]
pub enum CodecError<P: PhyLayer + Debug> {
    Encode,
    Decode,
    Physical(P::Error),
}

impl<P: PhyLayer + Debug> From<postcard::Error> for CodecError<P> {
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

impl<P: PhyLayer + Debug> From<error::Error> for CodecError<P> {
    fn from(e: error::Error) -> Self {
        match e {
            error::Error::DataLinkEncode => CodecError::Encode,
            error::Error::DataLinkDecode => CodecError::Decode,
            _ => CodecError::Decode,
        }
    }
}

use core::fmt::Debug;

use crate::layer::transport::TransportLayer;

#[derive(Debug)]
pub enum AppLayerImplError<T: TransportLayer + Debug> {
    Error,
    Transport(T::Error),
}

impl<P: TransportLayer + Debug> From<postcard::Error> for AppLayerImplError<P> {
    fn from(e: postcard::Error) -> Self {
        match e {
            postcard::Error::SerializeBufferFull
            | postcard::Error::SerdeSerCustom
            | postcard::Error::CollectStrError
            | postcard::Error::WontImplement
            | postcard::Error::NotYetImplemented => AppLayerImplError::Error,

            _ => AppLayerImplError::Error,
        }
    }
}

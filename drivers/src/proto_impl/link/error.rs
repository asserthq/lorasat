use core::fmt::Debug;

#[derive(defmt::Format, Debug, PartialEq)]
pub enum LinkError<PE: Debug> {
    Encode,
    Decode,
    Physical(PE),
}

impl<PE: Debug> From<postcard::Error> for LinkError<PE> {
    fn from(e: postcard::Error) -> Self {
        match e {
            postcard::Error::SerializeBufferFull
            | postcard::Error::SerdeSerCustom
            | postcard::Error::CollectStrError
            | postcard::Error::WontImplement
            | postcard::Error::NotYetImplemented => LinkError::Encode,

            _ => LinkError::Decode,
        }
    }
}

use core::fmt;

#[derive(Debug, Clone, PartialEq, defmt::Format)]
pub enum Error {
    Spi,
    Timeout,
    InvalidAddress,
    UnalignedAddress,
    PageOverflow,
    PayloadTooLong,
    BufferTooSmall,
    BadRegion,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Spi => f.write_str("flash spi error"),
            Error::Timeout => f.write_str("flash busy timeout"),
            Error::InvalidAddress => f.write_str("flash address out of range"),
            Error::UnalignedAddress => f.write_str("flash address not sector aligned"),
            Error::PageOverflow => f.write_str("page program crosses page boundary"),
            Error::PayloadTooLong => f.write_str("record payload exceeds max size"),
            Error::BufferTooSmall => f.write_str("read buffer too small for record"),
            Error::BadRegion => f.write_str("ring region invalid"),
        }
    }
}

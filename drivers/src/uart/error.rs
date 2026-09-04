use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq, defmt::Format)]
pub enum Error {
    Read,
    Write,
    FrameTooLong,
    UnexpectedEof,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Read => f.write_str("uart read error"),
            Error::Write => f.write_str("uart write error"),
            Error::FrameTooLong => f.write_str("uart frame exceeds buffer"),
            Error::UnexpectedEof => f.write_str("uart stream ended before frame delimiter"),
        }
    }
}

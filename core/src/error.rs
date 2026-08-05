use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq, defmt::Format)]
pub enum Error {
    ProtocolError,
    Internal(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ProtocolError => f.write_str("protocol operation error"),
            Error::Internal(msg) => f.write_str(msg),
        }
    }
}

// ── From impls for foreign error types ──

impl From<postcard::Error> for Error {
    fn from(_: postcard::Error) -> Self {
        Error::ProtocolError
    }
}

use core::fmt;

use embedded_sdmmc::{Error as SdmmcError, FilenameError, SdCardError};

#[derive(Debug, Clone, defmt::Format)]
pub enum Error {
    Sdmmc(SdmmcError<SdCardError>),
    Device(SdCardError),
    Filename(FilenameError),
    NotInitialized,
    AlreadyInitialized,
    MaxFilesReached,
}

impl From<SdmmcError<SdCardError>> for Error {
    fn from(e: SdmmcError<SdCardError>) -> Self {
        Error::Sdmmc(e)
    }
}

impl From<SdCardError> for Error {
    fn from(e: SdCardError) -> Self {
        Error::Device(e)
    }
}

impl From<FilenameError> for Error {
    fn from(e: FilenameError) -> Self {
        Error::Filename(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sdmmc(e) => write!(f, "sdmmc: {:?}", e),
            Error::Device(e) => write!(f, "sd device: {:?}", e),
            Error::Filename(e) => write!(f, "filename: {:?}", e),
            Error::NotInitialized => f.write_str("sd logger not initialized"),
            Error::AlreadyInitialized => f.write_str("sd logger already initialized"),
            Error::MaxFilesReached => f.write_str("sd log rotation exhausted"),
        }
    }
}

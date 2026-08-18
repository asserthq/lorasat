use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Error {
    OpenVolume,
    OpenRootDir,
    OpenFile,
    Write,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::OpenVolume => f.write_str("Open volume failed"),
            Error::OpenRootDir => f.write_str("Open root dir failed"),
            Error::OpenFile => f.write_str("Open file failed"),
            Error::Write => f.write_str("sd write failed"),
        }
    }
}

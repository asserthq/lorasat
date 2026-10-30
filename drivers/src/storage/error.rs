use crate::flash;

#[derive(Clone, Debug, PartialEq, defmt::Format)]
pub enum StorageError {
    Full,
    EntryTooLong { length: usize, maximum: usize },
    BufferTooSmall { length: usize, required: usize },
    InvalidRegion,
    CorruptEntry,
    Flash(flash::Error),
}

impl From<flash::Error> for StorageError {
    fn from(error: flash::Error) -> Self {
        Self::Flash(error)
    }
}

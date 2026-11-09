use core::fmt::Debug;

#[derive(Clone, Copy, Debug, Eq, PartialEq, defmt::Format)]
pub struct StorageMetadata {
    length: usize,
}

impl StorageMetadata {
    pub const fn new(length: usize) -> Self {
        Self { length }
    }

    pub const fn length(self) -> usize {
        self.length
    }

    pub const fn is_empty(self) -> bool {
        self.length == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, defmt::Format)]
#[non_exhaustive]
pub enum StorageErrorKind {
    Full,
    EntryTooLong,
    BufferTooSmall,
    CorruptEntry,
    Media,
    Other,
}

#[allow(async_fn_in_trait)]
pub trait Storage {
    type Error: Debug;

    fn max_entry_size(&self) -> Option<usize> {
        None
    }

    fn error_kind(_error: &Self::Error) -> StorageErrorKind {
        StorageErrorKind::Other
    }

    async fn append(&mut self, entry: &[u8]) -> Result<(), Self::Error>;

    async fn peek(&mut self, buffer: &mut [u8]) -> Result<Option<StorageMetadata>, Self::Error>;

    async fn pop(&mut self) -> Result<bool, Self::Error>;

    async fn clear(&mut self) -> Result<(), Self::Error>;
}

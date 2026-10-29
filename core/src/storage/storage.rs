use core::fmt::Debug;

/// Metadata for the oldest entry currently exposed by [`Storage::peek`].
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

/// One bounded FIFO storage area containing opaque entries.
///
/// A concrete implementation may use RAM, flash, a circular partition, or an
/// in-memory test queue. Message boundaries are preserved. Serialization and
/// interpretation of entries belong to the subsystem using the storage.
///
/// [`Self::append`] stores one complete entry. It must return an error instead
/// of truncating the entry or silently evicting existing entries.
///
/// [`Self::peek`] copies the oldest entry without removing it. If `buffer` is
/// too small, it must return an error and retain the entry. [`Self::pop`]
/// removes that same oldest entry after the caller has processed it.
#[allow(async_fn_in_trait)]
pub trait Storage {
    type Error: Debug;

    async fn append(&mut self, entry: &[u8]) -> Result<(), Self::Error>;

    async fn peek(&mut self, buffer: &mut [u8]) -> Result<Option<StorageMetadata>, Self::Error>;

    async fn pop(&mut self) -> Result<bool, Self::Error>;

    async fn clear(&mut self) -> Result<(), Self::Error>;
}

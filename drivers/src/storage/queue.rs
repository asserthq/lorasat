use core::ops::Range;

use embedded_storage_async::nor_flash::{ErrorType, MultiwriteNorFlash, NorFlash, ReadNorFlash};
use sat_core::storage::{Storage, StorageErrorKind, StorageMetadata};
use sequential_storage::{
    cache::Cache,
    queue::{QueueConfig, QueueConfigError, QueueStorage},
};

use super::sequential::UncachedStorage;

pub struct BorrowedNorFlash<'a, F: NorFlash>(&'a mut F);

impl<'a, F: NorFlash> BorrowedNorFlash<'a, F> {
    pub fn new(flash: &'a mut F) -> Self {
        Self(flash)
    }

    pub fn into_inner(self) -> &'a mut F {
        self.0
    }
}

impl<F: NorFlash> ErrorType for BorrowedNorFlash<'_, F> {
    type Error = F::Error;
}

impl<F: NorFlash> ReadNorFlash for BorrowedNorFlash<'_, F> {
    const READ_SIZE: usize = F::READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.0.read(offset, bytes).await
    }

    fn capacity(&self) -> usize {
        self.0.capacity()
    }
}

impl<F: NorFlash> NorFlash for BorrowedNorFlash<'_, F> {
    const WRITE_SIZE: usize = F::WRITE_SIZE;
    const ERASE_SIZE: usize = F::ERASE_SIZE;

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.0.write(offset, bytes).await
    }

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        self.0.erase(from, to).await
    }
}

impl<F: MultiwriteNorFlash> MultiwriteNorFlash for BorrowedNorFlash<'_, F> {}

#[derive(Debug, PartialEq, Eq)]
pub enum SequentialQueueConfigError {
    OutOfBounds,
    Queue(QueueConfigError),
}

#[derive(Debug, PartialEq)]
pub enum SequentialQueueError<E> {
    Full,
    EntryTooLong { length: usize, maximum: usize },
    BufferTooSmall { length: usize, required: usize },
    Backend(sequential_storage::Error<E>),
}

impl<E> SequentialQueueError<E> {
    fn from_backend(error: sequential_storage::Error<E>, buffer_size: usize) -> Self {
        match error {
            sequential_storage::Error::FullStorage => Self::Full,
            sequential_storage::Error::BufferTooSmall(required) => Self::BufferTooSmall {
                length: buffer_size,
                required,
            },
            error => Self::Backend(error),
        }
    }
}

pub struct SequentialQueue<F: NorFlash, const BUFFER_SIZE: usize> {
    queue: QueueStorage<F, UncachedStorage>,
    scratch: [u8; BUFFER_SIZE],
}

impl<F: NorFlash, const BUFFER_SIZE: usize> SequentialQueue<F, BUFFER_SIZE> {
    pub fn new(flash: F, range: Range<u32>) -> Result<Self, SequentialQueueConfigError> {
        if range.start >= range.end || u64::from(range.end) > flash.capacity() as u64 {
            return Err(SequentialQueueConfigError::OutOfBounds);
        }
        let config = QueueConfig::<F>::try_new(range).map_err(SequentialQueueConfigError::Queue)?;
        Ok(Self::from_queue(QueueStorage::new(
            flash,
            config,
            Cache::new_uncached(),
        )))
    }

    pub(crate) fn from_queue(queue: QueueStorage<F, UncachedStorage>) -> Self {
        Self {
            queue,
            scratch: [0; BUFFER_SIZE],
        }
    }

    fn entry_limit() -> usize {
        let word = F::READ_SIZE.max(F::WRITE_SIZE);
        let buffer_limit = BUFFER_SIZE / word * word;
        let page_limit = F::ERASE_SIZE
            .saturating_sub(2 * word)
            .saturating_sub(QueueStorage::<F, UncachedStorage>::item_overhead_size() as usize);
        buffer_limit.min(page_limit).min(u16::MAX as usize)
    }

    pub fn into_inner(self) -> F {
        self.queue.destroy().0
    }
}

impl<F: MultiwriteNorFlash, const BUFFER_SIZE: usize> Storage for SequentialQueue<F, BUFFER_SIZE> {
    type Error = SequentialQueueError<F::Error>;

    fn max_entry_size(&self) -> Option<usize> {
        Some(Self::entry_limit())
    }

    fn error_kind(error: &Self::Error) -> StorageErrorKind {
        match error {
            SequentialQueueError::Full => StorageErrorKind::Full,
            SequentialQueueError::EntryTooLong { .. } => StorageErrorKind::EntryTooLong,
            SequentialQueueError::BufferTooSmall { .. } => StorageErrorKind::BufferTooSmall,
            SequentialQueueError::Backend(error) => match error {
                sequential_storage::Error::FullStorage => StorageErrorKind::Full,
                sequential_storage::Error::BufferTooSmall(_) => StorageErrorKind::BufferTooSmall,
                sequential_storage::Error::Storage { .. } => StorageErrorKind::Media,
                sequential_storage::Error::Corrupted { .. } => StorageErrorKind::CorruptEntry,
                sequential_storage::Error::ItemTooBig => StorageErrorKind::EntryTooLong,
                _ => StorageErrorKind::Other,
            },
        }
    }

    async fn append(&mut self, entry: &[u8]) -> Result<(), Self::Error> {
        let maximum = Self::entry_limit();
        if entry.len() > maximum {
            return Err(SequentialQueueError::EntryTooLong {
                length: entry.len(),
                maximum,
            });
        }
        self.queue
            .push(entry, false)
            .await
            .map_err(|error| SequentialQueueError::from_backend(error, BUFFER_SIZE))
    }

    async fn peek(&mut self, buffer: &mut [u8]) -> Result<Option<StorageMetadata>, Self::Error> {
        let Some(entry) = self
            .queue
            .peek(&mut self.scratch)
            .await
            .map_err(|error| SequentialQueueError::from_backend(error, BUFFER_SIZE))?
        else {
            return Ok(None);
        };
        if buffer.len() < entry.len() {
            return Err(SequentialQueueError::BufferTooSmall {
                length: buffer.len(),
                required: entry.len(),
            });
        }
        buffer[..entry.len()].copy_from_slice(entry);
        Ok(Some(StorageMetadata::new(entry.len())))
    }

    async fn pop(&mut self) -> Result<bool, Self::Error> {
        self.queue
            .pop(&mut self.scratch)
            .await
            .map(|entry| entry.is_some())
            .map_err(|error| SequentialQueueError::from_backend(error, BUFFER_SIZE))
    }

    async fn clear(&mut self) -> Result<(), Self::Error> {
        self.queue
            .erase_all()
            .await
            .map_err(|error| SequentialQueueError::from_backend(error, BUFFER_SIZE))
    }
}

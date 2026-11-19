use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use sat_core::storage::{QueueErrorKind, QueueMetadata, QueueStorage};

use super::{RamStorage, RamStorageStats};

pub struct SharedQueue<M: RawMutex, Q> {
    queue: Mutex<M, Q>,
}

impl<M: RawMutex, Q: QueueStorage> SharedQueue<M, Q> {
    pub const fn new(queue: Q) -> Self {
        Self {
            queue: Mutex::new(queue),
        }
    }

    pub fn split(&mut self) -> (SharedQueueHandle<'_, M, Q>, SharedQueueHandle<'_, M, Q>) {
        (
            SharedQueueHandle { queue: &self.queue },
            SharedQueueHandle { queue: &self.queue },
        )
    }
}

pub struct SharedQueueHandle<'a, M: RawMutex, Q> {
    queue: &'a Mutex<M, Q>,
}

impl<M: RawMutex, Q: QueueStorage> QueueStorage for SharedQueueHandle<'_, M, Q> {
    type Error = Q::Error;

    fn max_entry_size(&self) -> Option<usize> {
        self.queue
            .try_lock()
            .ok()
            .and_then(|queue| queue.max_entry_size())
    }

    fn error_kind(error: &Self::Error) -> QueueErrorKind {
        Q::error_kind(error)
    }

    async fn append(&mut self, entry: &[u8]) -> Result<(), Self::Error> {
        self.queue.lock().await.append(entry).await
    }

    async fn peek(&mut self, buffer: &mut [u8]) -> Result<Option<QueueMetadata>, Self::Error> {
        self.queue.lock().await.peek(buffer).await
    }

    async fn pop(&mut self) -> Result<bool, Self::Error> {
        self.queue.lock().await.pop().await
    }

    async fn clear(&mut self) -> Result<(), Self::Error> {
        self.queue.lock().await.clear().await
    }
}

impl<M: RawMutex, const ENTRY_SIZE: usize, const BLOCK_COUNT: usize>
    SharedQueueHandle<'_, M, RamStorage<ENTRY_SIZE, BLOCK_COUNT>>
{
    pub async fn stats(&self) -> RamStorageStats {
        self.queue.lock().await.stats()
    }
}

use heapless::{Deque, Vec};
use sat_core::storage::{ConfigKey, KeyValueStorage, Storage, StorageMetadata};

use super::StorageError;

/// Fixed-capacity in-memory implementation of one logical [`Storage`] area.
pub struct RamStorage<const ENTRY_SIZE: usize, const DEPTH: usize> {
    entries: Deque<Vec<u8, ENTRY_SIZE>, DEPTH>,
}

impl<const ENTRY_SIZE: usize, const DEPTH: usize> RamStorage<ENTRY_SIZE, DEPTH> {
    pub const fn new() -> Self {
        Self {
            entries: Deque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<const ENTRY_SIZE: usize, const DEPTH: usize> Default for RamStorage<ENTRY_SIZE, DEPTH> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const ENTRY_SIZE: usize, const DEPTH: usize> Storage for RamStorage<ENTRY_SIZE, DEPTH> {
    type Error = StorageError;

    async fn append(&mut self, entry: &[u8]) -> Result<(), Self::Error> {
        let entry = Vec::from_slice(entry).map_err(|_| StorageError::EntryTooLong {
            length: entry.len(),
            maximum: ENTRY_SIZE,
        })?;
        self.entries
            .push_back(entry)
            .map_err(|_| StorageError::Full)
    }

    async fn peek(&mut self, buffer: &mut [u8]) -> Result<Option<StorageMetadata>, Self::Error> {
        let Some(entry) = self.entries.front() else {
            return Ok(None);
        };
        if buffer.len() < entry.len() {
            return Err(StorageError::BufferTooSmall {
                length: buffer.len(),
                required: entry.len(),
            });
        }

        buffer[..entry.len()].copy_from_slice(entry);
        Ok(Some(StorageMetadata::new(entry.len())))
    }

    async fn pop(&mut self) -> Result<bool, Self::Error> {
        Ok(self.entries.pop_front().is_some())
    }

    async fn clear(&mut self) -> Result<(), Self::Error> {
        self.entries.clear();
        Ok(())
    }
}

struct ConfigValue<const VALUE_SIZE: usize> {
    key: ConfigKey,
    value: Vec<u8, VALUE_SIZE>,
}

/// Fixed-capacity in-memory implementation used for configuration and tests.
pub struct RamKeyValueStorage<const VALUE_SIZE: usize, const KEYS: usize> {
    entries: [Option<ConfigValue<VALUE_SIZE>>; KEYS],
}

impl<const VALUE_SIZE: usize, const KEYS: usize> RamKeyValueStorage<VALUE_SIZE, KEYS> {
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; KEYS],
        }
    }

    fn find(&self, key: ConfigKey) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|entry| entry.key == key))
    }
}

impl<const VALUE_SIZE: usize, const KEYS: usize> Default for RamKeyValueStorage<VALUE_SIZE, KEYS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const VALUE_SIZE: usize, const KEYS: usize> KeyValueStorage
    for RamKeyValueStorage<VALUE_SIZE, KEYS>
{
    type Error = StorageError;

    async fn store(&mut self, key: ConfigKey, value: &[u8]) -> Result<(), Self::Error> {
        let value = Vec::from_slice(value).map_err(|_| StorageError::EntryTooLong {
            length: value.len(),
            maximum: VALUE_SIZE,
        })?;

        if let Some(index) = self.find(key) {
            self.entries[index] = Some(ConfigValue { key, value });
            return Ok(());
        }

        let index = self
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(StorageError::Full)?;
        self.entries[index] = Some(ConfigValue { key, value });
        Ok(())
    }

    async fn load(
        &mut self,
        key: ConfigKey,
        buffer: &mut [u8],
    ) -> Result<Option<usize>, Self::Error> {
        let Some(index) = self.find(key) else {
            return Ok(None);
        };
        let value = &self.entries[index]
            .as_ref()
            .expect("located configuration entry must exist")
            .value;
        if buffer.len() < value.len() {
            return Err(StorageError::BufferTooSmall {
                length: buffer.len(),
                required: value.len(),
            });
        }

        buffer[..value.len()].copy_from_slice(value);
        Ok(Some(value.len()))
    }

    async fn remove(&mut self, key: ConfigKey) -> Result<bool, Self::Error> {
        let Some(index) = self.find(key) else {
            return Ok(false);
        };
        self.entries[index] = None;
        Ok(true)
    }
}

use heapless::Vec;
use sat_core::storage::{Key, MapStorage, QueueErrorKind, QueueMetadata, QueueStorage};

use super::StorageError;

const LENGTH_SIZE: usize = core::mem::size_of::<u32>();

struct RamBlock<const ENTRY_SIZE: usize> {
    data: [u8; ENTRY_SIZE],
    overhead: [u8; LENGTH_SIZE],
}

impl<const ENTRY_SIZE: usize> RamBlock<ENTRY_SIZE> {
    const fn new() -> Self {
        Self {
            data: [0; ENTRY_SIZE],
            overhead: [0; LENGTH_SIZE],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RamStorageStats {
    pub capacity_bytes: usize,
    pub used_bytes: usize,
    pub free_bytes: usize,
    pub records: usize,
}

pub struct RamStorage<const ENTRY_SIZE: usize, const BLOCK_COUNT: usize> {
    blocks: [RamBlock<ENTRY_SIZE>; BLOCK_COUNT],
    head: usize,
    used: usize,
    records: usize,
}

impl<const ENTRY_SIZE: usize, const BLOCK_COUNT: usize> RamStorage<ENTRY_SIZE, BLOCK_COUNT> {
    const BLOCK_SIZE: usize = ENTRY_SIZE + LENGTH_SIZE;
    pub const CAPACITY_BYTES: usize = Self::BLOCK_SIZE * BLOCK_COUNT;

    pub const fn new() -> Self {
        assert!(
            ENTRY_SIZE <= u32::MAX as usize,
            "RAM entry length must fit a u32"
        );
        let _ = Self::CAPACITY_BYTES;
        Self {
            blocks: [const { RamBlock::<ENTRY_SIZE>::new() }; BLOCK_COUNT],
            head: 0,
            used: 0,
            records: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.records
    }

    pub fn is_empty(&self) -> bool {
        self.records == 0
    }

    pub fn stats(&self) -> RamStorageStats {
        RamStorageStats {
            capacity_bytes: Self::CAPACITY_BYTES,
            used_bytes: self.used,
            free_bytes: Self::CAPACITY_BYTES - self.used,
            records: self.records,
        }
    }

    fn advance(position: usize, length: usize) -> usize {
        let remaining = Self::CAPACITY_BYTES - position;
        if length < remaining {
            position + length
        } else {
            length - remaining
        }
    }

    fn segment(&self, position: usize) -> &[u8] {
        let block = &self.blocks[position / Self::BLOCK_SIZE];
        let offset = position % Self::BLOCK_SIZE;
        if offset < ENTRY_SIZE {
            &block.data[offset..]
        } else {
            &block.overhead[offset - ENTRY_SIZE..]
        }
    }

    fn segment_mut(&mut self, position: usize) -> &mut [u8] {
        let block = &mut self.blocks[position / Self::BLOCK_SIZE];
        let offset = position % Self::BLOCK_SIZE;
        if offset < ENTRY_SIZE {
            &mut block.data[offset..]
        } else {
            &mut block.overhead[offset - ENTRY_SIZE..]
        }
    }

    fn write_at(&mut self, mut position: usize, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let target = self.segment_mut(position);
            let copied = bytes.len().min(target.len());
            target[..copied].copy_from_slice(&bytes[..copied]);
            bytes = &bytes[copied..];
            position = Self::advance(position, copied);
        }
    }

    fn read_at(&self, mut position: usize, mut buffer: &mut [u8]) {
        while !buffer.is_empty() {
            let source = self.segment(position);
            let copied = buffer.len().min(source.len());
            let (part, remaining) = buffer.split_at_mut(copied);
            part.copy_from_slice(&source[..copied]);
            buffer = remaining;
            position = Self::advance(position, copied);
        }
    }

    fn front_length(&self) -> Result<Option<usize>, StorageError> {
        if self.is_empty() {
            return Ok(None);
        }
        if self.used < LENGTH_SIZE {
            return Err(StorageError::CorruptEntry);
        }
        let mut header = [0; LENGTH_SIZE];
        self.read_at(self.head, &mut header);
        let length =
            usize::try_from(u32::from_le_bytes(header)).map_err(|_| StorageError::CorruptEntry)?;
        if length > ENTRY_SIZE || length > self.used - LENGTH_SIZE {
            return Err(StorageError::CorruptEntry);
        }
        Ok(Some(length))
    }
}

impl<const ENTRY_SIZE: usize, const BLOCK_COUNT: usize> Default
    for RamStorage<ENTRY_SIZE, BLOCK_COUNT>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const ENTRY_SIZE: usize, const BLOCK_COUNT: usize> QueueStorage
    for RamStorage<ENTRY_SIZE, BLOCK_COUNT>
{
    type Error = StorageError;

    fn max_entry_size(&self) -> Option<usize> {
        Some(ENTRY_SIZE)
    }

    fn error_kind(error: &Self::Error) -> QueueErrorKind {
        match error {
            StorageError::Full => QueueErrorKind::Full,
            StorageError::EntryTooLong { .. } => QueueErrorKind::EntryTooLong,
            StorageError::BufferTooSmall { .. } => QueueErrorKind::BufferTooSmall,
            StorageError::CorruptEntry => QueueErrorKind::CorruptEntry,
            StorageError::Flash(_) => QueueErrorKind::Media,
            StorageError::InvalidRegion => QueueErrorKind::Other,
        }
    }

    async fn append(&mut self, entry: &[u8]) -> Result<(), Self::Error> {
        if entry.len() > ENTRY_SIZE {
            return Err(StorageError::EntryTooLong {
                length: entry.len(),
                maximum: ENTRY_SIZE,
            });
        }
        let required = LENGTH_SIZE + entry.len();
        if required > Self::CAPACITY_BYTES - self.used {
            return Err(StorageError::Full);
        }

        let tail = Self::advance(self.head, self.used);
        self.write_at(tail, &(entry.len() as u32).to_le_bytes());
        self.write_at(Self::advance(tail, LENGTH_SIZE), entry);
        self.used += required;
        self.records += 1;
        Ok(())
    }

    async fn peek(&mut self, buffer: &mut [u8]) -> Result<Option<QueueMetadata>, Self::Error> {
        let Some(length) = self.front_length()? else {
            return Ok(None);
        };
        if buffer.len() < length {
            return Err(StorageError::BufferTooSmall {
                length: buffer.len(),
                required: length,
            });
        }
        self.read_at(Self::advance(self.head, LENGTH_SIZE), &mut buffer[..length]);
        Ok(Some(QueueMetadata::new(length)))
    }

    async fn pop(&mut self) -> Result<bool, Self::Error> {
        let Some(length) = self.front_length()? else {
            return Ok(false);
        };
        let consumed = LENGTH_SIZE + length;
        self.head = Self::advance(self.head, consumed);
        self.used -= consumed;
        self.records -= 1;
        if self.is_empty() {
            self.head = 0;
        }
        Ok(true)
    }

    async fn clear(&mut self) -> Result<(), Self::Error> {
        self.head = 0;
        self.used = 0;
        self.records = 0;
        Ok(())
    }
}

struct ConfigValue<const VALUE_SIZE: usize> {
    key: Key,
    value: Vec<u8, VALUE_SIZE>,
}

pub struct RamKeyValueStorage<const VALUE_SIZE: usize, const KEYS: usize> {
    entries: [Option<ConfigValue<VALUE_SIZE>>; KEYS],
}

impl<const VALUE_SIZE: usize, const KEYS: usize> RamKeyValueStorage<VALUE_SIZE, KEYS> {
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; KEYS],
        }
    }

    fn find(&self, key: Key) -> Option<usize> {
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

impl<const VALUE_SIZE: usize, const KEYS: usize> MapStorage
    for RamKeyValueStorage<VALUE_SIZE, KEYS>
{
    type Error = StorageError;

    async fn store(&mut self, key: Key, value: &[u8]) -> Result<(), Self::Error> {
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

    async fn load(&mut self, key: Key, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error> {
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

    async fn remove(&mut self, key: Key) -> Result<bool, Self::Error> {
        let Some(index) = self.find(key) else {
            return Ok(false);
        };
        self.entries[index] = None;
        Ok(true)
    }
}

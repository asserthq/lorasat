use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embedded_hal::{delay::DelayNs, spi::SpiDevice};
use sat_core::storage::{Storage, StorageMetadata};

use crate::flash::{CAPACITY, SECTOR_SIZE, W25Q64, ring::crc32};

use super::StorageError;

const MAGIC: u32 = 0x5354_4F52;
const FORMAT_VERSION: u8 = 1;
const HEADER_SIZE: usize = 16;
const MAGIC_OFFSET: usize = 0;
const VERSION_OFFSET: usize = 4;
const LENGTH_OFFSET: usize = 6;
const SEQUENCE_OFFSET: usize = 8;
const CRC_OFFSET: usize = 12;

pub const FLASH_ENTRY_MAX_SIZE: usize = SECTOR_SIZE as usize - HEADER_SIZE;

pub type SharedFlash<RawMutexType, Spi, Delay> = Mutex<RawMutexType, W25Q64<Spi, Delay>>;

#[derive(Clone, Copy)]
struct EntryHeader {
    length: usize,
    sequence: u32,
    crc: u32,
}

/// Simple power-loss-aware flash queue implementing one logical [`Storage`]
/// partition.
///
/// One entry occupies one complete flash sector. This wastes capacity but keeps
/// erase ownership, recovery, `peek`, and `pop` easy to inspect. Multiple
/// partitions can share one [`W25Q64`] through [`SharedFlash`].
///
/// The current `W25Q64` driver uses blocking SPI and polling delays. Holding the
/// async mutex therefore blocks the executor during flash operations; this
/// implementation is suitable for bring-up, not final real-time scheduling.
pub struct FlashStorage<'a, RawMutexType, Spi, Delay>
where
    RawMutexType: RawMutex,
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    flash: &'a SharedFlash<RawMutexType, Spi, Delay>,
    base: u32,
    sector_count: usize,
    head_sector: usize,
    tail_sector: usize,
    entry_count: usize,
    next_sequence: u32,
}

impl<'a, RawMutexType, Spi, Delay> FlashStorage<'a, RawMutexType, Spi, Delay>
where
    RawMutexType: RawMutex,
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    pub async fn mount(
        flash: &'a SharedFlash<RawMutexType, Spi, Delay>,
        base: u32,
        length: u32,
    ) -> Result<Self, StorageError> {
        if length == 0
            || !base.is_multiple_of(SECTOR_SIZE)
            || !length.is_multiple_of(SECTOR_SIZE)
            || base.checked_add(length).is_none_or(|end| end > CAPACITY)
        {
            return Err(StorageError::InvalidRegion);
        }

        let sector_count = (length / SECTOR_SIZE) as usize;
        let mut minimum: Option<(u32, usize)> = None;
        let mut maximum: Option<(u32, usize)> = None;
        let mut entry_count = 0;
        let mut flash_guard = flash.lock().await;

        for sector in 0..sector_count {
            let address = sector_address(base, sector);
            let Some(header) = read_header(&mut flash_guard, address)? else {
                continue;
            };
            verify_entry(&mut flash_guard, address, header)?;
            entry_count += 1;

            if minimum.is_none_or(|(sequence, _)| header.sequence < sequence) {
                minimum = Some((header.sequence, sector));
            }
            if maximum.is_none_or(|(sequence, _)| header.sequence > sequence) {
                maximum = Some((header.sequence, sector));
            }
        }
        drop(flash_guard);

        let head_sector = minimum.map_or(0, |(_, sector)| sector);
        let tail_sector = maximum.map_or(0, |(_, sector)| (sector + 1) % sector_count);
        let next_sequence = maximum.map_or(1, |(sequence, _)| sequence.wrapping_add(1));

        Ok(Self {
            flash,
            base,
            sector_count,
            head_sector,
            tail_sector,
            entry_count,
            next_sequence,
        })
    }

    pub const fn len(&self) -> usize {
        self.entry_count
    }

    pub const fn capacity(&self) -> usize {
        self.sector_count
    }

    pub const fn is_empty(&self) -> bool {
        self.entry_count == 0
    }
}

impl<RawMutexType, Spi, Delay> Storage for FlashStorage<'_, RawMutexType, Spi, Delay>
where
    RawMutexType: RawMutex,
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    type Error = StorageError;

    async fn append(&mut self, entry: &[u8]) -> Result<(), Self::Error> {
        if entry.len() > FLASH_ENTRY_MAX_SIZE {
            return Err(StorageError::EntryTooLong {
                length: entry.len(),
                maximum: FLASH_ENTRY_MAX_SIZE,
            });
        }
        if self.entry_count == self.sector_count {
            return Err(StorageError::Full);
        }

        let was_empty = self.entry_count == 0;
        let mut flash = self.flash.lock().await;
        let sector = find_free_sector(&mut flash, self.base, self.sector_count, self.tail_sector)?
            .ok_or(StorageError::Full)?;
        let address = sector_address(self.base, sector);

        flash.erase_sector(address)?;
        flash.program(address + HEADER_SIZE as u32, entry)?;

        let header = encode_header(EntryHeader {
            length: entry.len(),
            sequence: self.next_sequence,
            crc: crc32(entry),
        });
        flash.program(address, &header)?;
        drop(flash);

        if was_empty {
            self.head_sector = sector;
        }
        self.tail_sector = (sector + 1) % self.sector_count;
        self.entry_count += 1;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        Ok(())
    }

    async fn peek(&mut self, buffer: &mut [u8]) -> Result<Option<StorageMetadata>, Self::Error> {
        if self.entry_count == 0 {
            return Ok(None);
        }

        let address = sector_address(self.base, self.head_sector);
        let mut flash = self.flash.lock().await;
        let header = read_header(&mut flash, address)?.ok_or(StorageError::CorruptEntry)?;
        if buffer.len() < header.length {
            return Err(StorageError::BufferTooSmall {
                length: buffer.len(),
                required: header.length,
            });
        }

        flash.read(address + HEADER_SIZE as u32, &mut buffer[..header.length])?;
        if crc32(&buffer[..header.length]) != header.crc {
            return Err(StorageError::CorruptEntry);
        }

        Ok(Some(StorageMetadata::new(header.length)))
    }

    async fn pop(&mut self) -> Result<bool, Self::Error> {
        if self.entry_count == 0 {
            return Ok(false);
        }

        let old_head = self.head_sector;
        let mut flash = self.flash.lock().await;
        flash.erase_sector(sector_address(self.base, old_head))?;
        let remaining = self.entry_count - 1;
        let next_head = if remaining == 0 {
            self.tail_sector
        } else {
            find_next_entry(
                &mut flash,
                self.base,
                self.sector_count,
                (old_head + 1) % self.sector_count,
            )?
            .ok_or(StorageError::CorruptEntry)?
        };
        drop(flash);

        self.entry_count = remaining;
        self.head_sector = next_head;
        Ok(true)
    }

    async fn clear(&mut self) -> Result<(), Self::Error> {
        let mut flash = self.flash.lock().await;
        for sector in 0..self.sector_count {
            flash.erase_sector(sector_address(self.base, sector))?;
        }
        drop(flash);

        self.head_sector = 0;
        self.tail_sector = 0;
        self.entry_count = 0;
        self.next_sequence = 1;
        Ok(())
    }
}

fn sector_address(base: u32, sector: usize) -> u32 {
    base + sector as u32 * SECTOR_SIZE
}

fn find_free_sector<Spi, Delay>(
    flash: &mut W25Q64<Spi, Delay>,
    base: u32,
    sector_count: usize,
    start: usize,
) -> Result<Option<usize>, StorageError>
where
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    for offset in 0..sector_count {
        let sector = (start + offset) % sector_count;
        if read_header(flash, sector_address(base, sector))?.is_none() {
            return Ok(Some(sector));
        }
    }
    Ok(None)
}

fn find_next_entry<Spi, Delay>(
    flash: &mut W25Q64<Spi, Delay>,
    base: u32,
    sector_count: usize,
    start: usize,
) -> Result<Option<usize>, StorageError>
where
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    for offset in 0..sector_count {
        let sector = (start + offset) % sector_count;
        if read_header(flash, sector_address(base, sector))?.is_some() {
            return Ok(Some(sector));
        }
    }
    Ok(None)
}

fn read_header<Spi, Delay>(
    flash: &mut W25Q64<Spi, Delay>,
    address: u32,
) -> Result<Option<EntryHeader>, StorageError>
where
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    let mut bytes = [0_u8; HEADER_SIZE];
    flash.read(address, &mut bytes)?;

    if bytes.iter().all(|byte| *byte == u8::MAX) {
        return Ok(None);
    }
    if read_u32(&bytes, MAGIC_OFFSET) != MAGIC || bytes[VERSION_OFFSET] != FORMAT_VERSION {
        return Ok(None);
    }

    let length = read_u16(&bytes, LENGTH_OFFSET) as usize;
    if length > FLASH_ENTRY_MAX_SIZE {
        return Err(StorageError::CorruptEntry);
    }

    Ok(Some(EntryHeader {
        length,
        sequence: read_u32(&bytes, SEQUENCE_OFFSET),
        crc: read_u32(&bytes, CRC_OFFSET),
    }))
}

fn verify_entry<Spi, Delay>(
    flash: &mut W25Q64<Spi, Delay>,
    address: u32,
    header: EntryHeader,
) -> Result<(), StorageError>
where
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    let mut crc = crate::flash::ring::Crc32::new();
    let mut read_address = address + HEADER_SIZE as u32;
    let mut remaining = header.length;
    let mut buffer = [0_u8; 64];

    while remaining > 0 {
        let length = remaining.min(buffer.len());
        flash.read(read_address, &mut buffer[..length])?;
        crc.update(&buffer[..length]);
        remaining -= length;
        read_address += length as u32;
    }

    if crc.finalize() != header.crc {
        return Err(StorageError::CorruptEntry);
    }
    Ok(())
}

fn encode_header(header: EntryHeader) -> [u8; HEADER_SIZE] {
    let mut bytes = [0_u8; HEADER_SIZE];
    write_u32(&mut bytes, MAGIC_OFFSET, MAGIC);
    bytes[VERSION_OFFSET] = FORMAT_VERSION;
    write_u16(&mut bytes, LENGTH_OFFSET, header.length as u16);
    write_u32(&mut bytes, SEQUENCE_OFFSET, header.sequence);
    write_u32(&mut bytes, CRC_OFFSET, header.crc);
    bytes
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

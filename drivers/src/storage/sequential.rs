use core::ops::Range;

use embedded_storage_async::nor_flash::NorFlash;
use sequential_storage::{
    cache::{Cache, Uncached},
    map::{MapConfig, MapConfigError, MapStorage, PostcardValue},
    queue::{QueueConfig, QueueConfigError, QueueStorage},
};
use serde::{Deserialize, Serialize};

use super::queue::{BorrowedNorFlash, SequentialQueue};

pub type UncachedStorage<K = ()> = Cache<Uncached, Uncached, Uncached, K>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageArea {
    Telemetry,
    ClientPackets,
    Settings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageLayout {
    pub telemetry: Range<u32>,
    pub client_packets: Range<u32>,
    pub settings: Range<u32>,
}

#[derive(Debug)]
pub enum LayoutError {
    OutOfBounds(StorageArea),
    Overlap(StorageArea, StorageArea),
    Queue(StorageArea, QueueConfigError),
    Map(MapConfigError),
}

pub struct SatelliteStorage<F: NorFlash> {
    flash: F,
    layout: StorageLayout,
}

impl<F: NorFlash> SatelliteStorage<F> {
    pub fn new(flash: F, layout: StorageLayout) -> Result<Self, LayoutError> {
        let areas = [
            (StorageArea::Telemetry, &layout.telemetry),
            (StorageArea::ClientPackets, &layout.client_packets),
            (StorageArea::Settings, &layout.settings),
        ];
        for (area, range) in areas {
            if range.start >= range.end || range.end as u64 > flash.capacity() as u64 {
                return Err(LayoutError::OutOfBounds(area));
            }
        }
        for (index, (area, range)) in areas.iter().enumerate() {
            for (other_area, other_range) in &areas[index + 1..] {
                if range.start < other_range.end && other_range.start < range.end {
                    return Err(LayoutError::Overlap(*area, *other_area));
                }
            }
        }
        QueueConfig::<F>::try_new(layout.telemetry.clone())
            .map_err(|error| LayoutError::Queue(StorageArea::Telemetry, error))?;
        QueueConfig::<F>::try_new(layout.client_packets.clone())
            .map_err(|error| LayoutError::Queue(StorageArea::ClientPackets, error))?;
        MapConfig::<F>::try_new(layout.settings.clone()).map_err(LayoutError::Map)?;
        Ok(Self { flash, layout })
    }

    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    pub fn telemetry(&mut self) -> QueueStorage<&mut F, UncachedStorage> {
        QueueStorage::new(
            &mut self.flash,
            QueueConfig::new(self.layout.telemetry.clone()),
            Cache::new_uncached(),
        )
    }

    pub fn client_packets(&mut self) -> QueueStorage<&mut F, UncachedStorage> {
        QueueStorage::new(
            &mut self.flash,
            QueueConfig::new(self.layout.client_packets.clone()),
            Cache::new_uncached(),
        )
    }

    pub fn telemetry_queue<const BUFFER_SIZE: usize>(
        &mut self,
    ) -> SequentialQueue<BorrowedNorFlash<'_, F>, BUFFER_SIZE> {
        SequentialQueue::from_queue(QueueStorage::new(
            BorrowedNorFlash::new(&mut self.flash),
            QueueConfig::new(self.layout.telemetry.clone()),
            Cache::new_uncached(),
        ))
    }

    pub fn client_packet_queue<const BUFFER_SIZE: usize>(
        &mut self,
    ) -> SequentialQueue<BorrowedNorFlash<'_, F>, BUFFER_SIZE> {
        SequentialQueue::from_queue(QueueStorage::new(
            BorrowedNorFlash::new(&mut self.flash),
            QueueConfig::new(self.layout.client_packets.clone()),
            Cache::new_uncached(),
        ))
    }

    pub fn settings(&mut self) -> MapStorage<u16, &mut F, UncachedStorage<u16>> {
        MapStorage::new(
            &mut self.flash,
            MapConfig::new(self.layout.settings.clone()),
            Cache::new_uncached(),
        )
    }

    pub async fn save_telemetry<T: Serialize>(
        &mut self,
        value: &T,
        scratch: &mut [u8],
    ) -> Result<(), TypedStorageError<F::Error>> {
        let bytes = postcard::to_slice(value, scratch).map_err(TypedStorageError::Codec)?;
        self.telemetry()
            .push(bytes, true)
            .await
            .map_err(TypedStorageError::Storage)
    }

    pub async fn save_client_packet<T: Serialize>(
        &mut self,
        value: &T,
        scratch: &mut [u8],
    ) -> Result<(), TypedStorageError<F::Error>> {
        let bytes = postcard::to_slice(value, scratch).map_err(TypedStorageError::Codec)?;
        self.client_packets()
            .push(bytes, false)
            .await
            .map_err(TypedStorageError::Storage)
    }

    pub async fn peek_telemetry<'d, T: Deserialize<'d>>(
        &mut self,
        scratch: &'d mut [u8],
    ) -> Result<Option<T>, TypedStorageError<F::Error>> {
        match self
            .telemetry()
            .peek(scratch)
            .await
            .map_err(TypedStorageError::Storage)?
        {
            Some(bytes) => decode_record(bytes)
                .map(Some)
                .map_err(TypedStorageError::Codec),
            None => Ok(None),
        }
    }

    pub async fn peek_client_packet<'d, T: Deserialize<'d>>(
        &mut self,
        scratch: &'d mut [u8],
    ) -> Result<Option<T>, TypedStorageError<F::Error>> {
        match self
            .client_packets()
            .peek(scratch)
            .await
            .map_err(TypedStorageError::Storage)?
        {
            Some(bytes) => decode_record(bytes)
                .map(Some)
                .map_err(TypedStorageError::Codec),
            None => Ok(None),
        }
    }

    pub fn into_inner(self) -> F {
        self.flash
    }
}

#[derive(Debug)]
pub enum TypedStorageError<E> {
    Storage(sequential_storage::Error<E>),
    Codec(postcard::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Postcard<T>(pub T);

impl<'d, T: Serialize + Deserialize<'d>> PostcardValue<'d> for Postcard<T> {}

pub fn decode_record<'d, T: Deserialize<'d>>(bytes: &'d [u8]) -> Result<T, postcard::Error> {
    let (value, remaining) = postcard::take_from_bytes(bytes)?;
    if !remaining.is_empty() {
        return Err(postcard::Error::DeserializeBadEncoding);
    }
    Ok(value)
}

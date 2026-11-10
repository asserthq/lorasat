use std::ops::Range;

use embedded_storage_async::nor_flash::{
    ErrorType, MultiwriteNorFlash, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};
use futures::executor::block_on;
use sat_drivers::storage::{
    LayoutError, Postcard, SatelliteStorage, StorageArea, StorageLayout, TypedStorageError,
    decode_record,
};
use sequential_storage::{
    Error,
    map::MapConfigError,
    queue::{QueueConfig, QueueConfigError},
};
use serde::{Deserialize, Serialize};

const SECTOR: usize = 4096;
const CAPACITY: usize = 64 * 1024;
const CONFIG_KEY: u16 = 7;
const CHECKPOINT_KEY: u16 = 9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FlashError {
    OutOfBounds,
    NotAligned,
    PowerCut,
}

impl NorFlashError for FlashError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Self::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            Self::NotAligned => NorFlashErrorKind::NotAligned,
            Self::PowerCut => NorFlashErrorKind::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Counters {
    reads: usize,
    writes: usize,
    erases: usize,
    programmed: usize,
    erased: usize,
}

#[derive(Clone)]
struct Nor {
    bytes: Vec<u8>,
    counts: Counters,

    budget: Option<usize>,
}

impl Nor {
    fn new() -> Self {
        Self {
            bytes: vec![0xff; CAPACITY],
            counts: Counters::default(),
            budget: None,
        }
    }

    fn range(&self, offset: u32, len: usize, alignment: usize) -> Result<Range<usize>, FlashError> {
        let start = offset as usize;
        let end = start.checked_add(len).ok_or(FlashError::OutOfBounds)?;
        if end > self.bytes.len() {
            return Err(FlashError::OutOfBounds);
        }
        if !start.is_multiple_of(alignment) || !len.is_multiple_of(alignment) {
            return Err(FlashError::NotAligned);
        }
        Ok(start..end)
    }

    fn mutation_byte(&mut self) -> Result<(), FlashError> {
        if let Some(left) = &mut self.budget {
            if *left == 0 {
                return Err(FlashError::PowerCut);
            }
            *left -= 1;
        }
        Ok(())
    }

    fn power_cycle(mut self) -> Self {
        self.budget = None;
        self
    }
}

impl ErrorType for Nor {
    type Error = FlashError;
}

impl ReadNorFlash for Nor {
    const READ_SIZE: usize = 1;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        let range = self.range(offset, bytes.len(), Self::READ_SIZE)?;
        self.counts.reads += 1;
        bytes.copy_from_slice(&self.bytes[range]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.bytes.len()
    }
}

impl NorFlash for Nor {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = SECTOR;

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        let range = self.range(offset, bytes.len(), Self::WRITE_SIZE)?;
        self.counts.writes += 1;
        for (index, byte) in range.zip(bytes) {
            self.mutation_byte()?;
            self.bytes[index] &= byte;
            self.counts.programmed += 1;
        }
        Ok(())
    }

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        let len = to.checked_sub(from).ok_or(FlashError::OutOfBounds)?;
        let range = self.range(from, len as usize, Self::ERASE_SIZE)?;
        self.counts.erases += 1;
        for index in range {
            self.mutation_byte()?;
            self.bytes[index] = 0xff;
            self.counts.erased += 1;
        }
        Ok(())
    }
}

impl MultiwriteNorFlash for Nor {}

type Storage = SatelliteStorage<Nor>;

fn layout() -> StorageLayout {
    StorageLayout {
        telemetry: 0..2 * SECTOR as u32,
        client_packets: 2 * SECTOR as u32..4 * SECTOR as u32,
        settings: 4 * SECTOR as u32..6 * SECTOR as u32,
    }
}

fn open(flash: Nor) -> Storage {
    SatelliteStorage::new(flash, layout()).unwrap()
}

fn reopen(storage: Storage) -> Storage {
    open(storage.into_inner().power_cycle())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Record {
    sequence: u32,
    payload: [u8; 12],
}

fn record(sequence: u32) -> Record {
    Record {
        sequence,
        payload: [sequence as u8; 12],
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Config {
    interval: u16,
    enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Checkpoint {
    sequence: u32,
}

async fn telemetry_records(storage: &mut Storage) -> Vec<Record> {
    let mut queue = storage.telemetry();
    let mut iter = queue.iter().await.unwrap();
    let mut scratch = [0; 64];
    let mut values = Vec::new();
    while let Some(entry) = iter.next(&mut scratch).await.unwrap() {
        values.push(decode_record(&entry).unwrap());
    }
    values
}

async fn client_records(storage: &mut Storage) -> Vec<Record> {
    let mut queue = storage.client_packets();
    let mut iter = queue.iter().await.unwrap();
    let mut scratch = [0; 64];
    let mut values = Vec::new();
    while let Some(entry) = iter.next(&mut scratch).await.unwrap() {
        values.push(decode_record(&entry).unwrap());
    }
    values
}

fn invalid_layout(edit: impl FnOnce(&mut StorageLayout)) -> LayoutError {
    let mut ranges = layout();
    edit(&mut ranges);
    SatelliteStorage::new(Nor::new(), ranges).err().unwrap()
}

#[test]
fn nor_model_checks_ranges_and_preserves_and_programmed_prefixes() {
    block_on(async {
        let mut flash = Nor::new();
        assert_eq!(
            flash.read(CAPACITY as u32, &mut [0]).await,
            Err(FlashError::OutOfBounds)
        );
        assert_eq!(
            flash.write(CAPACITY as u32, &[0]).await,
            Err(FlashError::OutOfBounds)
        );
        assert_eq!(
            flash.erase(SECTOR as u32, 0).await,
            Err(FlashError::OutOfBounds)
        );
        assert_eq!(
            flash.erase(0, CAPACITY as u32 + SECTOR as u32).await,
            Err(FlashError::OutOfBounds)
        );
        assert_eq!(
            flash.erase(1, SECTOR as u32).await,
            Err(FlashError::NotAligned)
        );
        assert_eq!(
            flash.erase(0, SECTOR as u32 - 1).await,
            Err(FlashError::NotAligned)
        );
        assert_eq!(flash.counts, Counters::default());
        flash.write(0, &[0x0f, 0xf0]).await.unwrap();
        flash.write(0, &[0xf0, 0xff]).await.unwrap();
        assert_eq!(&flash.bytes[..2], &[0, 0xf0]);
        flash.budget = Some(2);
        assert_eq!(flash.write(2, &[1, 2, 3]).await, Err(FlashError::PowerCut));
        assert_eq!(&flash.bytes[..5], &[0, 0xf0, 1, 2, 0xff]);
        assert_eq!(flash.counts.programmed, 6);
        let bytes = flash.bytes.clone();
        let mut flash = flash.power_cycle();
        assert_eq!(flash.bytes, bytes);
        flash.erase(0, SECTOR as u32).await.unwrap();
        assert!(flash.bytes[..SECTOR].iter().all(|byte| *byte == 0xff));
        assert_eq!(flash.counts.erased, SECTOR);
        flash.write(CAPACITY as u32 - 1, &[0x55]).await.unwrap();
        let mut last = [0];
        flash.read(CAPACITY as u32 - 1, &mut last).await.unwrap();
        assert_eq!(last, [0x55]);
    });
}

#[test]
fn layout_rejects_invalid_ranges_and_library_minima() {
    for area in [
        StorageArea::Telemetry,
        StorageArea::ClientPackets,
        StorageArea::Settings,
    ] {
        for reversed in [false, true] {
            let error = invalid_layout(|layout| {
                let range = match area {
                    StorageArea::Telemetry => &mut layout.telemetry,
                    StorageArea::ClientPackets => &mut layout.client_packets,
                    StorageArea::Settings => &mut layout.settings,
                };
                range.start = range.end + if reversed { SECTOR as u32 } else { 0 };
            });
            assert!(matches!(error, LayoutError::OutOfBounds(found) if found == area));
        }
        let error = invalid_layout(|layout| {
            let range = match area {
                StorageArea::Telemetry => &mut layout.telemetry,
                StorageArea::ClientPackets => &mut layout.client_packets,
                StorageArea::Settings => &mut layout.settings,
            };
            range.end = CAPACITY as u32 + SECTOR as u32;
        });
        assert!(matches!(error, LayoutError::OutOfBounds(found) if found == area));
    }
    assert!(matches!(
        invalid_layout(|l| l.client_packets.start -= SECTOR as u32),
        LayoutError::Overlap(StorageArea::Telemetry, StorageArea::ClientPackets)
    ));
    assert!(matches!(
        invalid_layout(|l| l.settings.start = SECTOR as u32),
        LayoutError::Overlap(StorageArea::Telemetry, StorageArea::Settings)
    ));
    assert!(matches!(
        invalid_layout(|l| l.settings.start -= SECTOR as u32),
        LayoutError::Overlap(StorageArea::ClientPackets, StorageArea::Settings)
    ));
    assert!(matches!(
        invalid_layout(|l| l.telemetry.start += 1),
        LayoutError::Queue(
            StorageArea::Telemetry,
            QueueConfigError::StartRangeNotAtPageBoundary
        )
    ));
    assert!(matches!(
        invalid_layout(|l| l.telemetry.end -= 1),
        LayoutError::Queue(
            StorageArea::Telemetry,
            QueueConfigError::EndRangeNotAtPageBoundary
        )
    ));
    assert!(matches!(
        invalid_layout(|l| l.client_packets.start += 1),
        LayoutError::Queue(
            StorageArea::ClientPackets,
            QueueConfigError::StartRangeNotAtPageBoundary
        )
    ));
    assert!(matches!(
        invalid_layout(|l| l.client_packets.end -= 1),
        LayoutError::Queue(
            StorageArea::ClientPackets,
            QueueConfigError::EndRangeNotAtPageBoundary
        )
    ));
    assert!(matches!(
        invalid_layout(|l| l.settings.start += 1),
        LayoutError::Map(MapConfigError::StartRangeNotAtPageBoundary)
    ));
    assert!(matches!(
        invalid_layout(|l| l.settings.end -= 1),
        LayoutError::Map(MapConfigError::EndRangeNotAtPageBoundary)
    ));
    assert!(matches!(
        invalid_layout(|l| l.settings.end -= SECTOR as u32),
        LayoutError::Map(MapConfigError::RangeTooSmall)
    ));

    assert!(matches!(
        QueueConfig::<Nor>::try_new(0..0),
        Err(QueueConfigError::RangeTooSmall)
    ));
    let minimum = StorageLayout {
        telemetry: 0..SECTOR as u32,
        client_packets: SECTOR as u32..2 * SECTOR as u32,
        settings: 2 * SECTOR as u32..4 * SECTOR as u32,
    };
    let storage = SatelliteStorage::new(Nor::new(), minimum.clone()).unwrap();
    assert_eq!(storage.layout(), &minimum);
    assert_eq!(storage.into_inner().counts, Counters::default());
}

#[test]
fn small_telemetry_records_pack_one_sector_and_iteration_does_not_pop() {
    block_on(async {
        let mut storage = open(Nor::new());
        let expected: Vec<_> = (0..16).map(record).collect();
        for value in &expected {
            storage.save_telemetry(value, &mut [0; 64]).await.unwrap();
        }
        let flash = storage.into_inner();
        assert!(flash.bytes[..SECTOR].iter().any(|byte| *byte != 0xff));
        assert!(flash.bytes[SECTOR..].iter().all(|byte| *byte == 0xff));
        assert_eq!(flash.counts.erases, 0);
        let snapshot = flash.clone();
        let mut storage = open(flash);
        for _ in 0..2 {
            assert_eq!(telemetry_records(&mut storage).await, expected);
            assert_eq!(
                storage
                    .peek_telemetry::<Record>(&mut [0; 64])
                    .await
                    .unwrap(),
                Some(record(0))
            );
        }
        let flash = storage.into_inner();
        assert_eq!(flash.bytes, snapshot.bytes);
        assert_eq!(flash.counts.writes, snapshot.counts.writes);
        assert_eq!(flash.counts.erases, snapshot.counts.erases);
        assert!(flash.counts.reads > snapshot.counts.reads);
    });
}

#[test]
fn telemetry_overwrite_leaves_subscriber_and_settings_bytes_untouched() {
    block_on(async {
        let mut storage = open(Nor::new());
        let packet = record(42);
        let config = Postcard(Config {
            interval: 60,
            enabled: true,
        });
        storage
            .save_client_packet(&packet, &mut [0; 64])
            .await
            .unwrap();
        storage
            .settings()
            .store_item(&mut [0; 64], &CONFIG_KEY, &config)
            .await
            .unwrap();
        let flash = storage.into_inner();
        let protected = flash.bytes[2 * SECTOR..].to_vec();
        let mut storage = open(flash);
        for sequence in 0..1024 {
            storage
                .save_telemetry(&record(sequence), &mut [0; 64])
                .await
                .unwrap();
        }
        let retained = telemetry_records(&mut storage).await;
        assert!(!retained.is_empty());
        assert!(retained[0].sequence > 0);
        assert_eq!(retained.last(), Some(&record(1023)));
        assert!(
            retained
                .windows(2)
                .all(|pair| pair[1].sequence == pair[0].sequence + 1)
        );
        assert!(
            retained
                .iter()
                .all(|value| *value == record(value.sequence))
        );
        let flash = storage.into_inner();
        assert!(
            flash.counts.erases > 0,
            "must actually rotate/overwrite telemetry"
        );
        assert_eq!(&flash.bytes[2 * SECTOR..], protected);
        let mut storage = open(flash.power_cycle());
        assert_eq!(client_records(&mut storage).await, vec![packet]);
        assert_eq!(
            storage
                .settings()
                .fetch_item::<Postcard<Config>>(&mut [0; 64], &CONFIG_KEY)
                .await
                .unwrap(),
            Some(config)
        );
    });
}

#[test]
fn client_queue_full_does_not_overwrite_oldest_or_other_committed_packets() {
    block_on(async {
        let mut storage = open(Nor::new());
        let mut committed = Vec::new();
        let mut full = false;
        for sequence in 0..1024 {
            let value = record(sequence);
            match storage.save_client_packet(&value, &mut [0; 64]).await {
                Ok(()) => committed.push(value),
                Err(TypedStorageError::Storage(Error::FullStorage)) => {
                    full = true;
                    break;
                }
                result => panic!("unexpected queue result: {result:?}"),
            }
        }
        assert!(full, "bounded fill must reach FullStorage");
        assert!(committed.len() > 1);
        let mut storage = reopen(storage);
        for _ in 0..2 {
            assert_eq!(
                storage
                    .peek_client_packet::<Record>(&mut [0; 64])
                    .await
                    .unwrap(),
                Some(record(0))
            );
            assert_eq!(client_records(&mut storage).await, committed);
            assert!(matches!(
                storage
                    .save_client_packet(&record(1024), &mut [0; 64])
                    .await,
                Err(TypedStorageError::Storage(Error::FullStorage))
            ));
        }
        assert_eq!(client_records(&mut storage).await, committed);
    });
}

#[test]
fn typed_serialization_failure_performs_no_flash_io_or_mutation() {
    block_on(async {
        let mut storage = open(Nor::new());
        storage
            .save_telemetry(&record(1), &mut [0; 64])
            .await
            .unwrap();
        storage
            .save_client_packet(&record(2), &mut [0; 64])
            .await
            .unwrap();
        let snapshot = storage.into_inner();
        let mut storage = open(snapshot.clone());
        assert!(matches!(
            storage.save_telemetry(&record(3), &mut [0; 1]).await,
            Err(TypedStorageError::Codec(
                postcard::Error::SerializeBufferFull
            ))
        ));
        assert!(matches!(
            storage.save_client_packet(&record(3), &mut [0; 1]).await,
            Err(TypedStorageError::Codec(
                postcard::Error::SerializeBufferFull
            ))
        ));
        let flash = storage.into_inner();
        assert_eq!(flash.bytes, snapshot.bytes);
        assert_eq!(flash.counts, snapshot.counts);
    });
}

#[test]
fn native_map_replacement_and_distinct_checkpoint_survive_reopen() {
    block_on(async {
        let mut storage = open(Nor::new());
        let old = Postcard(Config {
            interval: 60,
            enabled: true,
        });
        let new = Postcard(Config {
            interval: 3000,
            enabled: false,
        });
        let checkpoint = Postcard(Checkpoint { sequence: 12345 });
        storage
            .settings()
            .store_item(&mut [0; 64], &CONFIG_KEY, &old)
            .await
            .unwrap();
        storage
            .settings()
            .store_item(&mut [0; 64], &CHECKPOINT_KEY, &checkpoint)
            .await
            .unwrap();
        storage
            .settings()
            .store_item(&mut [0; 64], &CONFIG_KEY, &new)
            .await
            .unwrap();
        let mut storage = reopen(storage);
        for _ in 0..2 {
            assert_eq!(
                storage
                    .settings()
                    .fetch_item::<Postcard<Config>>(&mut [0; 64], &CONFIG_KEY)
                    .await
                    .unwrap(),
                Some(new)
            );
            assert_eq!(
                storage
                    .settings()
                    .fetch_item::<Postcard<Checkpoint>>(&mut [0; 64], &CHECKPOINT_KEY)
                    .await
                    .unwrap(),
                Some(checkpoint)
            );
        }
    });
}

#[test]
fn queue_recovers_every_byte_prefix_of_one_non_rotating_append() {
    block_on(async {
        let mut storage = open(Nor::new());
        let anchor = record(1);
        let candidate = record(2);
        let later = record(3);
        storage.save_telemetry(&anchor, &mut [0; 64]).await.unwrap();
        let mut seeded = storage.into_inner();
        seeded.counts = Counters::default();
        let mut complete = open(seeded.clone());
        complete
            .save_telemetry(&candidate, &mut [0; 64])
            .await
            .unwrap();
        let complete = complete.into_inner();
        let total = complete.counts.programmed;
        assert!(total > 0);
        assert_eq!(complete.counts.erases, 0);
        println!(
            "queue append: cut prefixes 0..={total} programmed bytes ({} cases), no erase/rotation",
            total + 1
        );
        for cut in 0..=total {
            let mut flash = seeded.clone();
            flash.budget = Some(cut);
            let mut storage = open(flash);
            let result = storage.save_telemetry(&candidate, &mut [0; 64]).await;
            if cut < total {
                assert!(
                    matches!(
                        result,
                        Err(TypedStorageError::Storage(Error::Storage {
                            value: FlashError::PowerCut,
                            ..
                        }))
                    ),
                    "cut {cut}: {result:?}"
                );
            } else {
                result.unwrap();
            }
            let flash = storage.into_inner();
            assert_eq!(flash.counts.programmed, cut, "cut {cut}");
            assert_eq!(flash.counts.erases, 0, "cut {cut}");
            let mut storage = open(flash.power_cycle());
            let mut recovered = telemetry_records(&mut storage).await;
            assert!(
                recovered == [anchor] || recovered == [anchor, candidate],
                "cut {cut}: {recovered:?}"
            );
            if cut == total {
                assert_eq!(recovered, [anchor, candidate]);
            }
            storage.save_telemetry(&later, &mut [0; 64]).await.unwrap();
            recovered.push(later);
            let mut storage = reopen(storage);
            assert_eq!(
                telemetry_records(&mut storage).await,
                recovered,
                "cut {cut}: append after recovery"
            );
        }
    });
}

#[test]
fn map_recovers_every_byte_prefix_of_one_non_rotating_update() {
    block_on(async {
        let mut storage = open(Nor::new());
        let old = Postcard(Config {
            interval: 60,
            enabled: true,
        });
        let candidate = Postcard(Config {
            interval: 3000,
            enabled: false,
        });
        let later = Postcard(Config {
            interval: 1000,
            enabled: true,
        });
        let checkpoint = Postcard(Checkpoint { sequence: 12345 });
        storage
            .settings()
            .store_item(&mut [0; 64], &CONFIG_KEY, &old)
            .await
            .unwrap();
        storage
            .settings()
            .store_item(&mut [0; 64], &CHECKPOINT_KEY, &checkpoint)
            .await
            .unwrap();
        let mut seeded = storage.into_inner();
        seeded.counts = Counters::default();
        let mut complete = open(seeded.clone());
        complete
            .settings()
            .store_item(&mut [0; 64], &CONFIG_KEY, &candidate)
            .await
            .unwrap();
        let complete = complete.into_inner();
        let total = complete.counts.programmed;
        assert!(total > 0);
        assert_eq!(complete.counts.erases, 0);
        println!(
            "map update: cut prefixes 0..={total} programmed bytes ({} cases), no erase/rotation",
            total + 1
        );
        for cut in 0..=total {
            let mut flash = seeded.clone();
            flash.budget = Some(cut);
            let mut storage = open(flash);
            let result = storage
                .settings()
                .store_item(&mut [0; 64], &CONFIG_KEY, &candidate)
                .await;
            if cut < total {
                assert!(
                    matches!(
                        result,
                        Err(Error::Storage {
                            value: FlashError::PowerCut,
                            ..
                        })
                    ),
                    "cut {cut}: {result:?}"
                );
            } else {
                result.unwrap();
            }
            let flash = storage.into_inner();
            assert_eq!(flash.counts.programmed, cut, "cut {cut}");
            assert_eq!(flash.counts.erases, 0, "cut {cut}");
            let mut storage = open(flash.power_cycle());
            let recovered = storage
                .settings()
                .fetch_item::<Postcard<Config>>(&mut [0; 64], &CONFIG_KEY)
                .await
                .unwrap();
            assert!(
                recovered == Some(old) || recovered == Some(candidate),
                "cut {cut}: {recovered:?}"
            );
            if cut == total {
                assert_eq!(recovered, Some(candidate));
            }
            assert_eq!(
                storage
                    .settings()
                    .fetch_item::<Postcard<Checkpoint>>(&mut [0; 64], &CHECKPOINT_KEY)
                    .await
                    .unwrap(),
                Some(checkpoint),
                "cut {cut}: checkpoint"
            );
            storage
                .settings()
                .store_item(&mut [0; 64], &CONFIG_KEY, &later)
                .await
                .unwrap();
            let mut storage = reopen(storage);
            assert_eq!(
                storage
                    .settings()
                    .fetch_item::<Postcard<Config>>(&mut [0; 64], &CONFIG_KEY)
                    .await
                    .unwrap(),
                Some(later),
                "cut {cut}: update after recovery"
            );
            assert_eq!(
                storage
                    .settings()
                    .fetch_item::<Postcard<Checkpoint>>(&mut [0; 64], &CHECKPOINT_KEY)
                    .await
                    .unwrap(),
                Some(checkpoint),
                "cut {cut}: retained checkpoint"
            );
        }
    });
}

#[test]
fn malformed_postcard_and_trailing_bytes_fail_without_deleting_records() {
    block_on(async {
        let mut scratch = [0; 64];
        let mut trailing = postcard::to_slice(&record(1), &mut scratch)
            .unwrap()
            .to_vec();
        trailing.push(0);
        for bytes in [vec![0x80], trailing] {
            assert!(decode_record::<Record>(&bytes).is_err());
            let mut storage = open(Nor::new());
            storage.telemetry().push(&bytes, false).await.unwrap();
            storage.client_packets().push(&bytes, false).await.unwrap();
            let snapshot = storage.into_inner();
            let mut storage = open(snapshot.clone().power_cycle());
            for _ in 0..2 {
                assert!(matches!(
                    storage.peek_telemetry::<Record>(&mut [0; 64]).await,
                    Err(TypedStorageError::Codec(_))
                ));
                assert!(matches!(
                    storage.peek_client_packet::<Record>(&mut [0; 64]).await,
                    Err(TypedStorageError::Codec(_))
                ));
                assert_eq!(
                    storage
                        .telemetry()
                        .peek(&mut scratch)
                        .await
                        .unwrap()
                        .unwrap(),
                    bytes.as_slice()
                );
                assert_eq!(
                    storage
                        .client_packets()
                        .peek(&mut scratch)
                        .await
                        .unwrap()
                        .unwrap(),
                    bytes.as_slice()
                );
            }
            let flash = storage.into_inner();
            assert_eq!(flash.bytes, snapshot.bytes);
            assert_eq!(flash.counts.writes, snapshot.counts.writes);
            assert_eq!(flash.counts.erases, snapshot.counts.erases);
        }
    });
}

async fn fifo_byte_prefix_recovery(remove: bool) {
    use sat_core::storage::{QueueStorage as _, QueueErrorKind};
    use sat_drivers::storage::SequentialQueue;
    use sequential_storage::{cache::Cache, queue::QueueStorage};

    let range = layout().client_packets;
    let mut queue = SequentialQueue::<_, 64>::new(Nor::new(), range.clone()).unwrap();
    queue.append(b"first").await.unwrap();
    queue.append(b"second").await.unwrap();
    let mut seeded = queue.into_inner();
    seeded.counts = Counters::default();

    let mut complete = SequentialQueue::<_, 64>::new(seeded.clone(), range.clone()).unwrap();
    if remove {
        assert!(complete.pop().await.unwrap());
    } else {
        complete.append(b"third").await.unwrap();
    }
    let complete = complete.into_inner();
    let total = complete.counts.programmed;
    assert!(total > 0);
    assert_eq!(complete.counts.erases, 0, "non-rotating operation only");

    let original = vec![b"first".to_vec(), b"second".to_vec()];
    let committed = if remove {
        vec![b"second".to_vec()]
    } else {
        vec![b"first".to_vec(), b"second".to_vec(), b"third".to_vec()]
    };
    for cut in 0..=total {
        let mut flash = seeded.clone();
        flash.budget = Some(cut);
        let mut queue = SequentialQueue::<_, 64>::new(flash, range.clone()).unwrap();
        let result = if remove {
            queue.pop().await.map(|removed| assert!(removed))
        } else {
            queue.append(b"third").await
        };
        if cut < total {
            assert_eq!(
                SequentialQueue::<Nor, 64>::error_kind(&result.unwrap_err()),
                QueueErrorKind::Media,
                "cut {cut}, remove={remove}"
            );
        } else {
            result.unwrap();
        }
        let flash = queue.into_inner();
        assert_eq!(flash.counts.programmed, cut);
        assert_eq!(flash.counts.erases, 0);
        assert_eq!(
            &flash.bytes[..range.start as usize],
            &seeded.bytes[..range.start as usize]
        );
        assert_eq!(
            &flash.bytes[range.end as usize..],
            &seeded.bytes[range.end as usize..]
        );

        let mut raw = QueueStorage::new(
            flash.power_cycle(),
            QueueConfig::new(range.clone()),
            Cache::new_uncached(),
        );
        let mut values = Vec::new();
        {
            let mut iter = raw.iter().await.unwrap();
            while let Some(entry) = iter.next(&mut [0; 64]).await.unwrap() {
                values.push(entry.to_vec());
            }
        }
        assert!(
            values == original || values == committed,
            "cut {cut}: {values:?}"
        );
        if cut == total {
            assert_eq!(values, committed);
        }
        let mut queue = SequentialQueue::<_, 64>::new(raw.destroy().0, range.clone()).unwrap();
        queue.append(b"after recovery").await.unwrap();
        values.push(b"after recovery".to_vec());
        for expected in values {
            let mut buffer = [0; 64];
            let metadata = queue.peek(&mut buffer).await.unwrap().unwrap();
            assert_eq!(&buffer[..metadata.length()], expected);
            assert!(queue.pop().await.unwrap());
        }
        assert!(queue.peek(&mut []).await.unwrap().is_none());
    }
}

#[test]
fn core_fifo_adapter_recovers_each_byte_prefix_of_non_rotating_append() {
    block_on(fifo_byte_prefix_recovery(false));
}

#[test]
fn core_fifo_adapter_reconciles_each_byte_prefix_of_pop_before_retrying() {
    block_on(fifo_byte_prefix_recovery(true));
}

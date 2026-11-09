use std::ops::Range;

use embedded_storage_async::nor_flash::{
    ErrorType, MultiwriteNorFlash, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};
use futures::executor::block_on;
use sat_core::storage::{Storage, StorageErrorKind, StorageMetadata};
use sat_drivers::storage::{
    RamStorage, SatelliteStorage, SequentialQueue, SequentialQueueConfigError,
    SequentialQueueError, StorageError, StorageLayout,
};
use sequential_storage::{
    Error as BackendError,
    cache::{Cache, Uncached},
    queue::{QueueConfig, QueueConfigError, QueueStorage},
};

const PAGE: usize = 256;
type NoCache = Cache<Uncached, Uncached, Uncached>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Io {
    Read,
    Write,
    Erase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlashError {
    Bounds,
    Alignment,
    Injected(Io),
}

impl NorFlashError for FlashError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Self::Bounds => NorFlashErrorKind::OutOfBounds,
            Self::Alignment => NorFlashErrorKind::NotAligned,
            Self::Injected(_) => NorFlashErrorKind::Other,
        }
    }
}

#[derive(Clone)]
struct Nor<const READ: usize = 1, const WRITE: usize = 1, const ERASE: usize = PAGE> {
    bytes: Vec<u8>,
    fail_next: Option<Io>,
    erased: Vec<Range<u32>>,
}

impl<const R: usize, const W: usize, const E: usize> Nor<R, W, E> {
    fn new(pages: usize) -> Self {
        Self {
            bytes: vec![0xff; pages * E],
            fail_next: None,
            erased: Vec::new(),
        }
    }

    fn access(
        &mut self,
        io: Io,
        offset: u32,
        length: usize,
        alignment: usize,
    ) -> Result<Range<usize>, FlashError> {
        let start = offset as usize;
        let end = start.checked_add(length).ok_or(FlashError::Bounds)?;
        if end > self.bytes.len() {
            return Err(FlashError::Bounds);
        }
        if !start.is_multiple_of(alignment) || !length.is_multiple_of(alignment) {
            return Err(FlashError::Alignment);
        }
        if self.fail_next == Some(io) {
            self.fail_next = None;
            return Err(FlashError::Injected(io));
        }
        Ok(start..end)
    }
}

impl<const R: usize, const W: usize, const E: usize> ErrorType for Nor<R, W, E> {
    type Error = FlashError;
}

impl<const R: usize, const W: usize, const E: usize> ReadNorFlash for Nor<R, W, E> {
    const READ_SIZE: usize = R;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        let range = self.access(Io::Read, offset, bytes.len(), R)?;
        bytes.copy_from_slice(&self.bytes[range]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.bytes.len()
    }
}

impl<const R: usize, const W: usize, const E: usize> NorFlash for Nor<R, W, E> {
    const WRITE_SIZE: usize = W;
    const ERASE_SIZE: usize = E;

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        let range = self.access(Io::Write, offset, bytes.len(), W)?;
        for (stored, byte) in self.bytes[range].iter_mut().zip(bytes) {
            *stored &= byte;
        }
        Ok(())
    }

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        let length = to.checked_sub(from).ok_or(FlashError::Bounds)? as usize;
        let range = self.access(Io::Erase, from, length, E)?;
        self.bytes[range].fill(0xff);
        self.erased.push(from..to);
        Ok(())
    }
}

impl<const R: usize, const W: usize, const E: usize> MultiwriteNorFlash for Nor<R, W, E> {}

async fn head<S: Storage>(storage: &mut S, expected: &[u8]) {
    let mut buffer = vec![0xa5; expected.len() + 3];
    let metadata = storage.peek(&mut buffer).await.unwrap().unwrap();
    assert_eq!(metadata.length(), expected.len());
    assert_eq!(metadata.is_empty(), expected.is_empty());
    assert_eq!(&buffer[..expected.len()], expected);
    assert_eq!(&buffer[expected.len()..], &[0xa5; 3]);
}

async fn empty<S: Storage>(storage: &mut S) {
    assert!(storage.peek(&mut []).await.unwrap().is_none());
    assert!(!storage.pop().await.unwrap());
}

async fn fill<S: Storage>(storage: &mut S) -> Vec<Vec<u8>> {
    let mut accepted = Vec::new();
    for sequence in 0_u32..1024 {
        let mut entry = sequence.to_le_bytes().to_vec();
        entry.extend_from_slice(&[0, 0xff, 0x55, 0xaa]);
        match storage.append(&entry).await {
            Ok(()) => accepted.push(entry),
            Err(error) => {
                assert_eq!(S::error_kind(&error), StorageErrorKind::Full);
                assert!(!accepted.is_empty());
                return accepted;
            }
        }
    }
    panic!("bounded test queue did not report Full");
}

async fn contract<S: Storage>(mut storage: S, maximum: usize) -> S {
    assert_eq!(storage.max_entry_size(), Some(maximum));
    empty(&mut storage).await;

    for entry in [b"a\0\xff".as_slice(), b"second", b"z"] {
        storage.append(entry).await.unwrap();
    }
    for entry in [b"a\0\xff".as_slice(), b"second", b"z"] {
        head(&mut storage, entry).await;
        head(&mut storage, entry).await;
        assert!(storage.pop().await.unwrap());
    }
    empty(&mut storage).await;

    storage.append(&[]).await.unwrap();
    storage.append(b"after empty").await.unwrap();
    for _ in 0..2 {
        let metadata = storage.peek(&mut []).await.unwrap().unwrap();
        assert!(metadata.is_empty());
        assert_eq!(metadata.length(), 0);
    }
    assert!(storage.pop().await.unwrap());
    head(&mut storage, b"after empty").await;
    assert!(storage.pop().await.unwrap());
    empty(&mut storage).await;

    storage.append(b"whole entry").await.unwrap();
    for _ in 0..2 {
        let mut small = [0x6d; 3];
        let error = storage.peek(&mut small).await.unwrap_err();
        assert_eq!(S::error_kind(&error), StorageErrorKind::BufferTooSmall);
        assert_eq!(small, [0x6d; 3], "failed peek partially copied an entry");
        let error = storage.peek(&mut []).await.unwrap_err();
        assert_eq!(S::error_kind(&error), StorageErrorKind::BufferTooSmall);
        head(&mut storage, b"whole entry").await;
    }
    assert!(storage.pop().await.unwrap());

    storage.append(b"anchor").await.unwrap();
    let error = storage.append(&vec![0x31; maximum + 1]).await.unwrap_err();
    assert_eq!(S::error_kind(&error), StorageErrorKind::EntryTooLong);
    let largest = vec![0x92; maximum];
    storage.append(&largest).await.unwrap();
    head(&mut storage, b"anchor").await;
    assert!(storage.pop().await.unwrap());
    head(&mut storage, &largest).await;
    assert!(storage.pop().await.unwrap());
    empty(&mut storage).await;

    let accepted = fill(&mut storage).await;
    for _ in 0..2 {
        let error = storage.append(b"overflow").await.unwrap_err();
        assert_eq!(S::error_kind(&error), StorageErrorKind::Full);
        head(&mut storage, &accepted[0]).await;
    }
    for entry in accepted {
        head(&mut storage, &entry).await;
        assert!(storage.pop().await.unwrap());
    }
    empty(&mut storage).await;

    storage.append(b"discard one").await.unwrap();
    storage.append(b"discard two").await.unwrap();
    storage.clear().await.unwrap();
    empty(&mut storage).await;
    storage.clear().await.unwrap();
    storage.append(b"reused").await.unwrap();
    head(&mut storage, b"reused").await;
    assert!(storage.pop().await.unwrap());
    empty(&mut storage).await;
    storage
}

#[test]
fn ram_obeys_fifo_contract() {
    block_on(contract(RamStorage::<32, 8>::new(), 32));
}

#[test]
fn flash_obeys_fifo_contract() {
    let queue = SequentialQueue::<_, 32>::new(Nor::<1, 1>::new(2), 0..2 * PAGE as u32).unwrap();
    block_on(contract(queue, 32));
}

#[test]
fn word_aligned_flash_obeys_fifo_contract() {
    let queue = SequentialQueue::<_, 16>::new(Nor::<4, 4>::new(2), 0..2 * PAGE as u32).unwrap();
    block_on(contract(queue, 16));
}

#[test]
fn existing_storage_implementations_can_use_provided_defaults() {
    struct Defaults(RamStorage<8, 2>);

    impl Storage for Defaults {
        type Error = StorageError;

        async fn append(&mut self, entry: &[u8]) -> Result<(), Self::Error> {
            self.0.append(entry).await
        }

        async fn peek(
            &mut self,
            buffer: &mut [u8],
        ) -> Result<Option<StorageMetadata>, Self::Error> {
            self.0.peek(buffer).await
        }

        async fn pop(&mut self) -> Result<bool, Self::Error> {
            self.0.pop().await
        }

        async fn clear(&mut self) -> Result<(), Self::Error> {
            self.0.clear().await
        }
    }

    let storage = Defaults(RamStorage::new());
    assert_eq!(storage.max_entry_size(), None);
    assert_eq!(
        Defaults::error_kind(&StorageError::Full),
        StorageErrorKind::Other
    );
}

#[test]
fn errors_have_portable_kinds() {
    for (error, kind) in [
        (StorageError::Full, StorageErrorKind::Full),
        (
            StorageError::EntryTooLong {
                length: 9,
                maximum: 8,
            },
            StorageErrorKind::EntryTooLong,
        ),
        (
            StorageError::BufferTooSmall {
                length: 3,
                required: 8,
            },
            StorageErrorKind::BufferTooSmall,
        ),
        (StorageError::CorruptEntry, StorageErrorKind::CorruptEntry),
        (StorageError::InvalidRegion, StorageErrorKind::Other),
        (
            StorageError::Flash(sat_drivers::flash::Error::Spi),
            StorageErrorKind::Media,
        ),
    ] {
        assert_eq!(RamStorage::<8, 2>::error_kind(&error), kind);
    }

    for (error, kind) in [
        (SequentialQueueError::Full, StorageErrorKind::Full),
        (
            SequentialQueueError::EntryTooLong {
                length: 17,
                maximum: 16,
            },
            StorageErrorKind::EntryTooLong,
        ),
        (
            SequentialQueueError::BufferTooSmall {
                length: 3,
                required: 7,
            },
            StorageErrorKind::BufferTooSmall,
        ),
        (
            SequentialQueueError::Backend(BackendError::FullStorage),
            StorageErrorKind::Full,
        ),
        (
            SequentialQueueError::Backend(BackendError::ItemTooBig),
            StorageErrorKind::EntryTooLong,
        ),
        (
            SequentialQueueError::Backend(BackendError::BufferTooSmall(20)),
            StorageErrorKind::BufferTooSmall,
        ),
        (
            SequentialQueueError::Backend(BackendError::Corrupted {}),
            StorageErrorKind::CorruptEntry,
        ),
        (
            SequentialQueueError::Backend(BackendError::LogicBug {}),
            StorageErrorKind::Other,
        ),
    ] {
        assert_eq!(SequentialQueue::<Nor, 16>::error_kind(&error), kind);
    }
}

#[test]
fn constructor_checks_bounds_before_upstream_configuration() {
    let capacity = (2 * PAGE) as u32;
    for (start, end) in [
        (0, 0),
        (1, 1),
        (capacity, capacity),
        (PAGE as u32, 0),
        (PAGE as u32 + 1, 1),
        (0, capacity + 1),
        (capacity + 1, capacity + PAGE as u32),
        (0, u32::MAX),
    ] {
        assert!(
            matches!(
                SequentialQueue::<_, 16>::new(Nor::<1, 1>::new(2), Range { start, end }),
                Err(SequentialQueueConfigError::OutOfBounds)
            ),
            "range {start}..{end}"
        );
    }

    for (range, expected) in [
        (
            1..PAGE as u32,
            QueueConfigError::StartRangeNotAtPageBoundary,
        ),
        (
            0..PAGE as u32 - 1,
            QueueConfigError::EndRangeNotAtPageBoundary,
        ),
        (
            0..PAGE as u32 / 2,
            QueueConfigError::EndRangeNotAtPageBoundary,
        ),
    ] {
        match SequentialQueue::<_, 16>::new(Nor::<1, 1>::new(2), range) {
            Err(SequentialQueueConfigError::Queue(error)) => assert_eq!(error, expected),
            _ => panic!("expected upstream configuration error {expected:?}"),
        }
    }

    for range in [0..PAGE as u32, PAGE as u32..capacity, 0..capacity] {
        let queue = SequentialQueue::<_, 16>::new(Nor::<1, 1>::new(2), range).unwrap();
        let flash = queue.into_inner();
        assert!(flash.bytes.iter().all(|byte| *byte == 0xff));
        assert!(flash.erased.is_empty());
    }
    assert!(matches!(
        SequentialQueue::<_, 16>::new(Nor::<4, 4, 16>::new(1), 0..16),
        Err(SequentialQueueConfigError::Queue(
            QueueConfigError::PagesTooSmall
        ))
    ));
    assert!(matches!(
        SequentialQueue::<_, 64>::new(Nor::<64, 64, 1024>::new(1), 0..1024),
        Err(SequentialQueueConfigError::Queue(
            QueueConfigError::WordSizeTooLarge
        ))
    ));
}

async fn limit<const BUFFER: usize, F: MultiwriteNorFlash>(flash: F, maximum: usize) {
    let end = flash.capacity() as u32;
    let mut queue = SequentialQueue::<_, BUFFER>::new(flash, 0..end).unwrap();
    assert_eq!(queue.max_entry_size(), Some(maximum));
    let payload = vec![0x79; maximum];
    queue.append(&payload).await.unwrap();
    let error = queue.append(&vec![0x47; maximum + 1]).await.unwrap_err();
    assert!(matches!(
        error,
        SequentialQueueError::EntryTooLong { length, maximum: found }
            if length == maximum + 1 && found == maximum
    ));
    head(&mut queue, &payload).await;
    assert!(queue.pop().await.unwrap());
    empty(&mut queue).await;
}

#[test]
fn advertised_limit_accounts_for_scratch_alignment_page_overhead_and_u16() {
    block_on(async {
        let overhead = QueueStorage::<Nor<4, 4>, NoCache>::item_overhead_size() as usize;
        let page_limit = PAGE - 2 * 4 - overhead;
        limit::<0, _>(Nor::<4, 4>::new(1), 0).await;
        limit::<3, _>(Nor::<4, 4>::new(1), 0).await;
        limit::<16, _>(Nor::<4, 4>::new(1), 16).await;
        limit::<19, _>(Nor::<4, 4>::new(1), 16).await;
        limit::<512, _>(Nor::<4, 4>::new(1), page_limit).await;
        limit::<19, _>(Nor::<1, 4>::new(1), 16).await;
        limit::<19, _>(Nor::<4, 1>::new(1), 16).await;
        limit::<65535, _>(Nor::<4, 4, 131072>::new(1), 65532).await;
        limit::<65536, _>(Nor::<4, 4, 131072>::new(1), u16::MAX as usize).await;
    });
}

#[test]
fn odd_caller_buffers_need_no_flash_alignment_and_receive_no_padding() {
    block_on(async {
        let mut queue = SequentialQueue::<_, 16>::new(Nor::<4, 4>::new(1), 0..PAGE as u32).unwrap();
        queue.append(b"seven!!").await.unwrap();
        let mut small = [0x51; 6];
        let error = queue.peek(&mut small).await.unwrap_err();
        assert!(matches!(
            error,
            SequentialQueueError::BufferTooSmall {
                length: 6,
                required: 7
            }
        ));
        assert_eq!(small, [0x51; 6]);

        let mut caller = [0xa5; 9];
        for _ in 0..2 {
            let metadata = queue.peek(&mut caller[1..8]).await.unwrap().unwrap();
            assert_eq!(metadata.length(), 7);
            assert_eq!(&caller[1..8], b"seven!!");
            assert_eq!(caller[0], 0xa5);
            assert_eq!(caller[8], 0xa5);
        }
        head(&mut queue, b"seven!!").await;
        assert!(queue.pop().await.unwrap());
        empty(&mut queue).await;
    });
}

#[test]
fn completed_append_pop_and_clear_survive_byte_preserving_reopen() {
    block_on(async {
        let range = PAGE as u32..3 * PAGE as u32;
        let mut queue = SequentialQueue::<_, 32>::new(Nor::<1, 1>::new(4), range.clone()).unwrap();
        queue.append(b"first\0\xff").await.unwrap();
        queue.append(b"second").await.unwrap();
        let flash = queue.into_inner();
        let appended = flash.bytes.clone();
        let mut queue = SequentialQueue::<_, 32>::new(flash, range.clone()).unwrap();
        head(&mut queue, b"first\0\xff").await;
        head(&mut queue, b"first\0\xff").await;
        let flash = queue.into_inner();
        assert_eq!(flash.bytes, appended);

        let mut queue = SequentialQueue::<_, 32>::new(flash, range.clone()).unwrap();
        assert!(queue.pop().await.unwrap());
        let flash = queue.into_inner();
        let popped = flash.bytes.clone();
        let mut queue = SequentialQueue::<_, 32>::new(flash, range.clone()).unwrap();
        head(&mut queue, b"second").await;
        let flash = queue.into_inner();
        assert_eq!(flash.bytes, popped);

        let mut queue = SequentialQueue::<_, 32>::new(flash, range.clone()).unwrap();
        queue.clear().await.unwrap();
        let flash = queue.into_inner();
        assert!(flash.bytes[PAGE..3 * PAGE].iter().all(|byte| *byte == 0xff));
        let cleared = flash.bytes.clone();
        let mut queue = SequentialQueue::<_, 32>::new(flash, range.clone()).unwrap();
        empty(&mut queue).await;
        let flash = queue.into_inner();
        assert_eq!(flash.bytes, cleared);

        let mut queue = SequentialQueue::<_, 32>::new(flash, range).unwrap();
        queue.append(b"after reopen").await.unwrap();
        head(&mut queue, b"after reopen").await;
    });
}

#[test]
fn consumed_sectors_wrap_without_overwriting_live_entries() {
    block_on(async {
        let range = 0..2 * PAGE as u32;
        let mut queue = SequentialQueue::<_, 256>::new(Nor::<1, 1>::new(2), range.clone()).unwrap();
        queue.append(&[0; 200]).await.unwrap();
        queue.append(&[1; 200]).await.unwrap();
        for sequence in 2_u8..12 {
            head(&mut queue, &[sequence - 2; 200]).await;
            assert!(queue.pop().await.unwrap());
            queue.append(&[sequence; 200]).await.unwrap();
            queue = SequentialQueue::new(queue.into_inner(), range.clone()).unwrap();
            head(&mut queue, &[sequence - 1; 200]).await;
        }
        head(&mut queue, &[10; 200]).await;
        assert!(queue.pop().await.unwrap());
        head(&mut queue, &[11; 200]).await;
        assert!(queue.pop().await.unwrap());
        empty(&mut queue).await;
        let flash = queue.into_inner();
        assert!(flash.erased.contains(&(0..PAGE as u32)));
        assert!(flash.erased.contains(&(PAGE as u32..2 * PAGE as u32)));
    });
}

#[test]
fn all_queue_operations_including_clear_stay_inside_the_region() {
    block_on(async {
        let mut flash = Nor::<1, 1>::new(5);
        flash.bytes[..PAGE].fill(0x35);
        flash.bytes[3 * PAGE..].fill(0xac);
        let before = flash.bytes.clone();
        let range = PAGE as u32..3 * PAGE as u32;
        let queue = SequentialQueue::<_, 32>::new(flash, range.clone()).unwrap();
        let mut queue = contract(queue, 32).await;
        queue.clear().await.unwrap();
        let flash = queue.into_inner();
        assert_eq!(&flash.bytes[..PAGE], &before[..PAGE]);
        assert_eq!(&flash.bytes[3 * PAGE..], &before[3 * PAGE..]);
        assert!(flash.bytes[PAGE..3 * PAGE].iter().all(|byte| *byte == 0xff));
        assert!(!flash.erased.is_empty());
        assert!(
            flash
                .erased
                .iter()
                .all(|erased| { erased.start >= range.start && erased.end <= range.end })
        );
    });
}

#[test]
fn satellite_queue_adapters_borrow_the_backend_and_preserve_raw_apis() {
    block_on(async {
        let mut flash = Nor::<1, 1>::new(6);
        flash.bytes[4 * PAGE..].fill(0x96);
        let layout = StorageLayout {
            telemetry: 0..2 * PAGE as u32,
            client_packets: 2 * PAGE as u32..4 * PAGE as u32,
            settings: 4 * PAGE as u32..6 * PAGE as u32,
        };
        let mut storage = SatelliteStorage::new(flash, layout.clone()).unwrap();
        storage
            .client_packet_queue::<32>()
            .append(b"client")
            .await
            .unwrap();
        storage
            .telemetry_queue::<32>()
            .append(b"telemetry")
            .await
            .unwrap();
        head(&mut storage.client_packet_queue::<32>(), b"client").await;
        head(&mut storage.telemetry_queue::<32>(), b"telemetry").await;
        assert_eq!(
            storage
                .client_packets()
                .peek(&mut [0; 32])
                .await
                .unwrap()
                .unwrap(),
            b"client"
        );
        storage
            .telemetry()
            .push(b"raw record", false)
            .await
            .unwrap();
        assert!(storage.telemetry_queue::<32>().pop().await.unwrap());
        head(&mut storage.telemetry_queue::<32>(), b"raw record").await;

        let flash = storage.into_inner();
        let protected = flash.bytes[2 * PAGE..].to_vec();
        let mut storage = SatelliteStorage::new(flash, layout.clone()).unwrap();
        storage.telemetry_queue::<32>().clear().await.unwrap();
        empty(&mut storage.telemetry_queue::<32>()).await;
        head(&mut storage.client_packet_queue::<32>(), b"client").await;
        let flash = storage.into_inner();
        assert_eq!(&flash.bytes[2 * PAGE..], protected.as_slice());

        let mut storage = SatelliteStorage::new(flash, layout).unwrap();
        assert!(storage.client_packet_queue::<32>().pop().await.unwrap());
        empty(&mut storage.client_packet_queue::<32>()).await;
        storage
            .telemetry_queue::<32>()
            .append(b"reused")
            .await
            .unwrap();
        head(&mut storage.telemetry_queue::<32>(), b"reused").await;
    });
}

#[test]
fn media_errors_keep_the_original_injected_io_error() {
    block_on(async {
        for (operation, failure) in [
            ("peek", Io::Read),
            ("append", Io::Write),
            ("pop", Io::Write),
            ("clear", Io::Erase),
        ] {
            let range = 0..2 * PAGE as u32;
            let mut queue =
                SequentialQueue::<_, 32>::new(Nor::<1, 1>::new(2), range.clone()).unwrap();
            queue.append(b"anchor").await.unwrap();
            let mut flash = queue.into_inner();
            flash.fail_next = Some(failure);
            let mut queue = SequentialQueue::<_, 32>::new(flash, range).unwrap();
            let result = match operation {
                "peek" => queue.peek(&mut [0; 32]).await.map(|_| ()),
                "append" => queue.append(b"later").await,
                "pop" => queue.pop().await.map(|_| ()),
                "clear" => queue.clear().await,
                _ => unreachable!(),
            };
            let error = result.unwrap_err();
            assert_eq!(
                SequentialQueue::<Nor, 32>::error_kind(&error),
                StorageErrorKind::Media
            );
            assert!(matches!(
                error,
                SequentialQueueError::Backend(BackendError::Storage {
                    value: FlashError::Injected(found), ..
                }) if found == failure
            ));
        }
    });
}

#[test]
fn an_oversized_raw_head_is_reported_and_retained_not_skipped_or_popped() {
    block_on(async {
        let range = 0..2 * PAGE as u32;
        let mut raw = QueueStorage::new(
            Nor::<4, 4>::new(2),
            QueueConfig::new(range.clone()),
            Cache::new_uncached(),
        );
        raw.push(&[0x69; 17], false).await.unwrap();
        raw.push(b"later", false).await.unwrap();
        let (flash, _) = raw.destroy();
        let original = flash.bytes.clone();
        let mut queue = SequentialQueue::<_, 16>::new(flash, range.clone()).unwrap();
        for _ in 0..2 {
            let mut caller = [0xb2; 32];
            let error = queue.peek(&mut caller).await.unwrap_err();
            assert_eq!(
                SequentialQueue::<Nor<4, 4>, 16>::error_kind(&error),
                StorageErrorKind::BufferTooSmall
            );
            assert_eq!(caller, [0xb2; 32]);
            let error = queue.pop().await.unwrap_err();
            assert_eq!(
                SequentialQueue::<Nor<4, 4>, 16>::error_kind(&error),
                StorageErrorKind::BufferTooSmall
            );
        }
        let flash = queue.into_inner();
        assert_eq!(flash.bytes, original);
        let mut raw = QueueStorage::new(flash, QueueConfig::new(range), Cache::new_uncached());
        assert_eq!(raw.peek(&mut [0; 32]).await.unwrap().unwrap(), &[0x69; 17]);
        assert_eq!(raw.pop(&mut [0; 32]).await.unwrap().unwrap(), &[0x69; 17]);
        assert_eq!(raw.peek(&mut [0; 32]).await.unwrap().unwrap(), b"later");
    });
}

#[test]
fn mock_enforces_alignment_and_bounds_and_models_nor_programming() {
    block_on(async {
        let mut flash = Nor::<4, 4>::new(2);
        assert_eq!(flash.read(1, &mut [0; 4]).await, Err(FlashError::Alignment));
        assert_eq!(flash.read(0, &mut [0; 3]).await, Err(FlashError::Alignment));
        assert_eq!(flash.write(1, &[0; 4]).await, Err(FlashError::Alignment));
        assert_eq!(flash.write(0, &[0; 3]).await, Err(FlashError::Alignment));
        assert_eq!(
            flash.erase(1, PAGE as u32).await,
            Err(FlashError::Alignment)
        );
        assert_eq!(
            flash.read((2 * PAGE) as u32, &mut [0; 4]).await,
            Err(FlashError::Bounds)
        );
        assert_eq!(
            flash.write((2 * PAGE) as u32, &[0; 4]).await,
            Err(FlashError::Bounds)
        );
        assert_eq!(
            flash.erase(0, (3 * PAGE) as u32).await,
            Err(FlashError::Bounds)
        );
        flash.write(0, &[0x0f; 4]).await.unwrap();
        flash.write(0, &[0xf3; 4]).await.unwrap();
        let mut bytes = [0; 4];
        flash.read(0, &mut bytes).await.unwrap();
        assert_eq!(bytes, [3; 4]);
        flash.bytes[PAGE..].fill(0x72);
        flash.erase(0, PAGE as u32).await.unwrap();
        assert!(flash.bytes[..PAGE].iter().all(|byte| *byte == 0xff));
        assert!(flash.bytes[PAGE..].iter().all(|byte| *byte == 0x72));
    });
}

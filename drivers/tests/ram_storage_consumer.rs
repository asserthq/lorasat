use std::{
    collections::VecDeque,
    fmt::Debug,
    future::poll_fn,
    pin::pin,
    task::{Context, Poll, Waker},
};

use futures::{Future, executor::block_on};
use sat_core::storage::QueueStorage;
use sat_drivers::storage::{RamStorage, StorageError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Acknowledgement {
    Confirmed,
    Missing,
}

trait Consumer {
    type Error: Debug;

    async fn consume(&mut self, entry: &[u8]) -> Result<Acknowledgement, Self::Error>;
}

#[derive(Debug, Eq, PartialEq)]
enum DeliveryOutcome {
    Empty,
    Unconfirmed,
    Delivered,
}

#[derive(Debug, Eq, PartialEq)]
enum DeliveryError<StorageError, ConsumerError> {
    Storage(StorageError),
    Consumer(ConsumerError),
    HeadMissing,
}

async fn deliver_next<S: QueueStorage, C: Consumer>(
    storage: &mut S,
    consumer: &mut C,
    buffer: &mut [u8],
) -> Result<DeliveryOutcome, DeliveryError<S::Error, C::Error>> {
    let Some(metadata) = storage.peek(buffer).await.map_err(DeliveryError::Storage)? else {
        return Ok(DeliveryOutcome::Empty);
    };

    match consumer
        .consume(&buffer[..metadata.length()])
        .await
        .map_err(DeliveryError::Consumer)?
    {
        Acknowledgement::Missing => Ok(DeliveryOutcome::Unconfirmed),
        Acknowledgement::Confirmed => {
            if storage.pop().await.map_err(DeliveryError::Storage)? {
                Ok(DeliveryOutcome::Delivered)
            } else {
                Err(DeliveryError::HeadMissing)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConsumerError {
    Disconnected,
}

type ConsumerResult = Result<Acknowledgement, ConsumerError>;

struct MockConsumer {
    expected: VecDeque<(Vec<u8>, ConsumerResult)>,
    attempts: Vec<Vec<u8>>,
    confirmed: Vec<Vec<u8>>,
}

impl MockConsumer {
    fn new<'a>(expected: impl IntoIterator<Item = (&'a [u8], ConsumerResult)>) -> Self {
        Self {
            expected: expected
                .into_iter()
                .map(|(entry, result)| (entry.to_vec(), result))
                .collect(),
            attempts: Vec::new(),
            confirmed: Vec::new(),
        }
    }

    fn assert_complete(&self) {
        assert!(
            self.expected.is_empty(),
            "expected delivery was not attempted"
        );
    }
}

impl Consumer for MockConsumer {
    type Error = ConsumerError;

    async fn consume(&mut self, entry: &[u8]) -> ConsumerResult {
        let (expected, result) = self.expected.pop_front().expect("unexpected delivery");
        assert_eq!(entry, expected);
        self.attempts.push(entry.to_vec());

        let mut yielded = false;
        poll_fn(|context| {
            if yielded {
                Poll::Ready(())
            } else {
                yielded = true;
                context.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;

        if result == Ok(Acknowledgement::Confirmed) {
            self.confirmed.push(entry.to_vec());
        }
        result
    }
}

async fn assert_head<S: QueueStorage>(storage: &mut S, expected: &[u8]) {
    let mut buffer = [0; 64];
    let metadata = storage.peek(&mut buffer).await.unwrap().unwrap();
    assert_eq!(&buffer[..metadata.length()], expected);
}

#[test]
fn missing_ack_retains_head_and_retry_delivers_in_fifo_order() {
    block_on(async {
        let first: &[u8] = b"\x00\xffsubscriber packet\x00";
        let second: &[u8] = b"second packet";
        let mut storage = RamStorage::<64, 2>::new();
        storage.append(first).await.unwrap();
        storage.append(second).await.unwrap();
        let mut consumer = MockConsumer::new([
            (first, Ok(Acknowledgement::Missing)),
            (first, Ok(Acknowledgement::Confirmed)),
            (second, Ok(Acknowledgement::Confirmed)),
        ]);
        let mut buffer = [0; 64];

        assert_eq!(
            deliver_next(&mut storage, &mut consumer, &mut buffer).await,
            Ok(DeliveryOutcome::Unconfirmed)
        );
        assert_eq!(storage.len(), 2);
        assert_head(&mut storage, first).await;
        assert!(consumer.confirmed.is_empty());

        assert_eq!(
            deliver_next(&mut storage, &mut consumer, &mut buffer).await,
            Ok(DeliveryOutcome::Delivered)
        );
        assert_eq!(storage.len(), 1);
        assert_head(&mut storage, second).await;
        assert_eq!(consumer.confirmed, [first.to_vec()]);

        assert_eq!(
            deliver_next(&mut storage, &mut consumer, &mut buffer).await,
            Ok(DeliveryOutcome::Delivered)
        );
        assert!(storage.is_empty());
        assert_eq!(consumer.confirmed, [first.to_vec(), second.to_vec()]);

        assert_eq!(
            deliver_next(&mut storage, &mut consumer, &mut buffer).await,
            Ok(DeliveryOutcome::Empty)
        );
        assert_eq!(
            consumer.attempts,
            [first.to_vec(), first.to_vec(), second.to_vec()]
        );
        consumer.assert_complete();
    });
}

#[test]
fn consumer_failure_keeps_record_for_a_later_attempt() {
    block_on(async {
        let packet: &[u8] = b"keep until confirmed";
        let mut storage = RamStorage::<64, 2>::new();
        storage.append(packet).await.unwrap();
        let mut consumer = MockConsumer::new([
            (packet, Err(ConsumerError::Disconnected)),
            (packet, Ok(Acknowledgement::Confirmed)),
        ]);
        let mut buffer = [0; 64];

        assert_eq!(
            deliver_next(&mut storage, &mut consumer, &mut buffer).await,
            Err(DeliveryError::Consumer(ConsumerError::Disconnected))
        );
        assert_eq!(storage.len(), 1);
        assert_head(&mut storage, packet).await;
        assert!(consumer.confirmed.is_empty());

        assert_eq!(
            deliver_next(&mut storage, &mut consumer, &mut buffer).await,
            Ok(DeliveryOutcome::Delivered)
        );
        assert!(storage.is_empty());
        assert_eq!(consumer.confirmed, [packet.to_vec()]);
        consumer.assert_complete();
    });
}

#[test]
fn small_buffer_never_calls_consumer_or_removes_record() {
    block_on(async {
        let packet: &[u8] = b"complete packet";
        let mut storage = RamStorage::<64, 2>::new();
        storage.append(packet).await.unwrap();
        let mut consumer = MockConsumer::new([(packet, Ok(Acknowledgement::Confirmed))]);
        let mut small = [0xa5; 2];

        assert_eq!(
            deliver_next(&mut storage, &mut consumer, &mut small).await,
            Err(DeliveryError::Storage(StorageError::BufferTooSmall {
                length: small.len(),
                required: packet.len(),
            }))
        );
        assert_eq!(small, [0xa5; 2]);
        assert_eq!(storage.len(), 1);
        assert!(consumer.attempts.is_empty());
        assert_head(&mut storage, packet).await;

        assert_eq!(
            deliver_next(&mut storage, &mut consumer, &mut [0; 64]).await,
            Ok(DeliveryOutcome::Delivered)
        );
        assert!(storage.is_empty());
        consumer.assert_complete();
    });
}

#[test]
fn empty_record_is_delivered_but_empty_queue_does_not_call_consumer() {
    block_on(async {
        let mut storage = RamStorage::<64, 2>::new();
        let mut consumer = MockConsumer::new([(b"".as_slice(), Ok(Acknowledgement::Confirmed))]);

        assert_eq!(
            deliver_next(&mut storage, &mut consumer, &mut []).await,
            Ok(DeliveryOutcome::Empty)
        );
        assert!(consumer.attempts.is_empty());

        storage.append(&[]).await.unwrap();
        assert_eq!(
            deliver_next(&mut storage, &mut consumer, &mut []).await,
            Ok(DeliveryOutcome::Delivered)
        );
        assert!(storage.is_empty());
        assert_eq!(consumer.confirmed, [Vec::<u8>::new()]);
        consumer.assert_complete();
    });
}

#[test]
fn canceling_consumer_wait_before_ack_keeps_ram_record() {
    let packet: &[u8] = b"pending acknowledgement";
    let mut storage = RamStorage::<64, 2>::new();
    block_on(storage.append(packet)).unwrap();
    let mut consumer = MockConsumer::new([
        (packet, Ok(Acknowledgement::Confirmed)),
        (packet, Ok(Acknowledgement::Confirmed)),
    ]);
    let mut buffer = [0; 64];

    {
        let mut delivery = pin!(deliver_next(&mut storage, &mut consumer, &mut buffer));
        let mut context = Context::from_waker(Waker::noop());
        assert_eq!(delivery.as_mut().poll(&mut context), Poll::Pending);
    }

    assert_eq!(storage.len(), 1);
    block_on(assert_head(&mut storage, packet));
    assert!(consumer.confirmed.is_empty());
    assert_eq!(consumer.attempts, [packet.to_vec()]);

    assert_eq!(
        block_on(deliver_next(&mut storage, &mut consumer, &mut buffer)),
        Ok(DeliveryOutcome::Delivered)
    );
    assert!(storage.is_empty());
    assert_eq!(consumer.confirmed, [packet.to_vec()]);
    assert_eq!(consumer.attempts, [packet.to_vec(), packet.to_vec()]);
    consumer.assert_complete();
}

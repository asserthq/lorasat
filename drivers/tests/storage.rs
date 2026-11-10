use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

use sat_core::storage::{Key, MapStorage, QueueStorage};
use sat_drivers::storage::{RamKeyValueStorage, RamStorage, StorageError};

fn run_ready<FutureType: Future>(future: FutureType) -> FutureType::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());

    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("test future unexpectedly pending"),
    }
}

#[test]
fn ram_storage_peeks_without_removing() {
    let mut storage = RamStorage::<8, 2>::new();
    run_ready(storage.append(b"first")).unwrap();
    run_ready(storage.append(b"second")).unwrap();

    let mut buffer = [0_u8; 8];
    let metadata = run_ready(storage.peek(&mut buffer)).unwrap().unwrap();
    assert_eq!(&buffer[..metadata.length()], b"first");
    assert_eq!(storage.len(), 2);

    assert!(run_ready(storage.pop()).unwrap());
    let metadata = run_ready(storage.peek(&mut buffer)).unwrap().unwrap();
    assert_eq!(&buffer[..metadata.length()], b"second");
}

#[test]
fn ram_storage_rejects_truncation_and_overflow() {
    let mut storage = RamStorage::<4, 1>::new();
    assert_eq!(
        run_ready(storage.append(b"large")),
        Err(StorageError::EntryTooLong {
            length: 5,
            maximum: 4,
        })
    );

    run_ready(storage.append(b"data")).unwrap();
    assert_eq!(run_ready(storage.append(b"next")), Err(StorageError::Full));

    let mut small = [0_u8; 2];
    assert_eq!(
        run_ready(storage.peek(&mut small)),
        Err(StorageError::BufferTooSmall {
            length: 2,
            required: 4,
        })
    );
    assert_eq!(storage.len(), 1);
}

#[test]
fn ram_key_value_storage_replaces_values() {
    let mut storage = RamKeyValueStorage::<8, 2>::new();
    let key = Key(7);
    run_ready(storage.store(key, b"old")).unwrap();
    run_ready(storage.store(key, b"new")).unwrap();

    let mut buffer = [0_u8; 8];
    let length = run_ready(storage.load(key, &mut buffer)).unwrap().unwrap();
    assert_eq!(&buffer[..length], b"new");

    assert!(run_ready(storage.remove(key)).unwrap());
    assert_eq!(run_ready(storage.load(key, &mut buffer)).unwrap(), None);
}

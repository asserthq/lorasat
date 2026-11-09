pub mod error;

pub mod ram;

pub mod queue;
pub mod sequential;

pub use error::StorageError;

pub use queue::{
    BorrowedNorFlash, SequentialQueue, SequentialQueueConfigError, SequentialQueueError,
};
pub use ram::{RamKeyValueStorage, RamStorage};

pub use sequential::{
    LayoutError, Postcard, SatelliteStorage, StorageArea, StorageLayout, TypedStorageError,
    decode_record,
};

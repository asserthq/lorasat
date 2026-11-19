pub mod error;
pub mod shared;

pub mod ram;

pub mod queue;
pub mod sequential;

pub use error::StorageError;
pub use shared::{SharedQueue, SharedQueueHandle};

pub use queue::{
    BorrowedNorFlash, SequentialQueue, SequentialQueueConfigError, SequentialQueueError,
};
pub use ram::{RamKeyValueStorage, RamStorage, RamStorageStats};

pub use sequential::{
    LayoutError, Postcard, SatelliteStorage, StorageArea, StorageLayout, TypedStorageError,
    decode_record,
};

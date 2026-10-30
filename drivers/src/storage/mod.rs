pub mod error;
pub mod flash;
pub mod ram;

pub use error::StorageError;
pub use flash::{FLASH_ENTRY_MAX_SIZE, FlashStorage, SharedFlash};
pub use ram::{RamKeyValueStorage, RamStorage};

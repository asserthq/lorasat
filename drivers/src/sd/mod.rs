pub mod error;
pub mod sdcard_logger;

pub use error::Error;
pub use sdcard_logger::SdCardLogger;

use embedded_sdmmc::{TimeSource, Timestamp};

#[derive(Debug, Clone, Copy)]
pub struct StaticTimeSource {
    timestamp: Timestamp,
}

impl StaticTimeSource {
    pub fn new(timestamp: Timestamp) -> Self {
        Self { timestamp }
    }
}

impl Default for StaticTimeSource {
    fn default() -> Self {
        Self {
            timestamp: Timestamp::from_calendar(2026, 1, 1, 0, 0, 0).unwrap(),
        }
    }
}

impl TimeSource for StaticTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        self.timestamp
    }
}

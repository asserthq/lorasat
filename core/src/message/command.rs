use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    RequestTelemetry,
    ChangeBeaconInterval(u32),
    SetTime(u64),
}

impl Command {
    pub fn try_encode<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Error> {
        postcard::to_slice(&self, buf).map_err(Error::from)
    }

    pub fn try_decode(data: &[u8]) -> Result<Self, Error> {
        postcard::from_bytes(data).map_err(Error::from)
    }
}

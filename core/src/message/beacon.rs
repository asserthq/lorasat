use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beacon {
    pub sat_addr: u32,
    pub interval_sec: u16,
    pub timestamp: u32,
}

impl Beacon {
    pub fn try_encode<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Error> {
        postcard::to_slice(&self, buf).map_err(Error::from)
    }

    pub fn try_decode(data: &[u8]) -> Result<Self, Error> {
        postcard::from_bytes(data).map_err(Error::from)
    }
}

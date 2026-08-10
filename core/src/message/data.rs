use heapless::Vec;
use serde::{Deserialize, Serialize};

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Data {
    pub kind: DataKind,
    pub src_addr: u32,
    pub dest_addr: u32,
    pub flags: u8,
    pub data: Vec<u8, 256>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum DataKind {
    Beacon = 0x01,
    DataAsp = 0x02,
    CommandAsp = 0x03,
    SatelliteData = 0x04,
    GroundCommand = 0x05,
}

impl Data {
    pub fn try_encode<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Error> {
        postcard::to_slice(&self, buf).map_err(Error::from)
    }

    pub fn try_decode(data: &[u8]) -> Result<Self, Error> {
        postcard::from_bytes(data).map_err(Error::from)
    }
}

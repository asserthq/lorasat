use super::data::Data;
use crate::error::Error;
use crate::message::Beacon;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    BeaconFrame(Beacon),
    DataFrame(Data),
}

impl Frame {
    pub fn try_encode<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Error> {
        postcard::to_slice(&self, buf).map_err(Error::from)
    }

    pub fn try_decode(data: &[u8]) -> Result<Self, Error> {
        postcard::from_bytes(data).map_err(Error::from)
    }
}

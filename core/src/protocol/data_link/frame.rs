use super::beacon::Beacon;
use super::data_frame::DataFrame;
use super::frame_type::FrameType;
use crate::protocol::data_link::error::ProtocolError;

use std::vec::Vec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Beacon(Beacon),
    Data(DataFrame),
}

impl Frame {
    pub fn try_decode(data: &[u8]) -> Result<Self, ProtocolError> {
        if data.is_empty() {
            return Err(ProtocolError::BufferTooShort);
        }

        let frame_type_id = data[0] & 0x0F;

        if frame_type_id == FrameType::Beacon as u8 {
            Ok(Frame::Beacon(Beacon::try_decode(data)?))
        } else {
            Ok(Frame::Data(DataFrame::try_decode(data)?))
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        match self {
            Frame::Beacon(b) => b.encode(),
            Frame::Data(d) => d.encode(),
        }
    }
}

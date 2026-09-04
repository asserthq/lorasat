use heapless::Vec;
use serde::{Deserialize, Serialize};

use super::error::TransportError;

pub const PROTOCOL_ID: u8 = 0x01;
pub const TYPE_START: u8 = 0x01;
pub const TYPE_DATA: u8 = 0x02;
pub const TYPE_ACK: u8 = 0x03;
pub const TYPE_CHECK: u8 = 0x04;

/// Maximum payload bytes per FRP data chunk.
///
/// Must fit inside `Data.data` (256 bytes) with postcard overhead
/// (~6 bytes for u16+u16+u8+Vec header) + 1 control byte = ~7 bytes total.
/// 240 leaves safe margin.
pub const MAX_CHUNK_PAYLOAD: usize = 240;
/// Maximum number of chunks per FRP session.
pub const MAX_CHUNKS: usize = 256;
/// Maximum bitmap bytes: ceil(MAX_CHUNKS / 8).
pub const MAX_BITMAP_BYTES: usize = (MAX_CHUNKS + 7) / 8;

fn make_control_byte(msg_type: u8) -> u8 {
    (PROTOCOL_ID << 4) | (msg_type & 0x0F)
}

// ── FrpStart ──

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrpStart {
    pub session_id: u16,
    pub total_chunks: u16,
    pub chunk_size: u16,
    pub message_length: u32,
    pub checksum: u32,
}

impl FrpStart {
    /// Encode: [control_byte][postcard body].
    pub fn try_encode<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], FrpError> {
        buf[0] = make_control_byte(TYPE_START);
        let written = {
            let used = postcard::to_slice(self, &mut buf[1..])?;
            used.len()
        };
        Ok(&mut buf[..1 + written])
    }

    /// Decode: verify control_byte, then postcard::from_bytes.
    pub fn try_decode(data: &[u8]) -> Result<Self, FrpError> {
        if data.is_empty() || data[0] != make_control_byte(TYPE_START) {
            return Err(FrpError::InvalidControlByte);
        }
        postcard::from_bytes(&data[1..]).map_err(Into::into)
    }
}

// ── FrpData ──

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrpData {
    pub session_id: u16,
    pub chunk_index: u16,
    pub flags: u8,
    pub payload: Vec<u8, MAX_CHUNK_PAYLOAD>,
}

impl FrpData {
    pub fn try_encode<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], FrpError> {
        buf[0] = make_control_byte(TYPE_DATA);
        let written = {
            let used = postcard::to_slice(self, &mut buf[1..])?;
            used.len()
        };
        Ok(&mut buf[..1 + written])
    }

    pub fn try_decode(data: &[u8]) -> Result<Self, FrpError> {
        if data.is_empty() || data[0] != make_control_byte(TYPE_DATA) {
            return Err(FrpError::InvalidControlByte);
        }
        postcard::from_bytes(&data[1..]).map_err(Into::into)
    }
}

// ── FrpAck ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrpAck {
    pub session_id: u16,
    pub chunk_index: u16,
    pub status: u8,
}

impl FrpAck {
    pub fn try_encode<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], FrpError> {
        buf[0] = make_control_byte(TYPE_ACK);
        let written = {
            let used = postcard::to_slice(self, &mut buf[1..])?;
            used.len()
        };
        Ok(&mut buf[..1 + written])
    }

    pub fn try_decode(data: &[u8]) -> Result<Self, FrpError> {
        if data.is_empty() || data[0] != make_control_byte(TYPE_ACK) {
            return Err(FrpError::InvalidControlByte);
        }
        postcard::from_bytes(&data[1..]).map_err(Into::into)
    }
}

// ── FrpCheck ──

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrpCheck {
    pub session_id: u16,
    pub total_chunks: u16,
    pub flags: u8,
    pub ack_bitmap: Vec<u8, MAX_BITMAP_BYTES>,
}

impl FrpCheck {
    pub fn try_encode<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], FrpError> {
        buf[0] = make_control_byte(TYPE_CHECK);
        let written = {
            let used = postcard::to_slice(self, &mut buf[1..])?;
            used.len()
        };
        Ok(&mut buf[..1 + written])
    }

    pub fn try_decode(data: &[u8]) -> Result<Self, FrpError> {
        if data.is_empty() || data[0] != make_control_byte(TYPE_CHECK) {
            return Err(FrpError::InvalidControlByte);
        }
        postcard::from_bytes(&data[1..]).map_err(Into::into)
    }
}

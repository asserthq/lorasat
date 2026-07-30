use super::error::ProtocolError;
use super::frame_type::FrameType;

use std::vec::Vec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFrame {
    pub frame_type: FrameType,
    pub src_addr: u32,
    pub dest_addr: u32,
    pub flags: u8,
    pub data: Vec<u8>,
}

impl DataFrame {
    pub const HEADER_SIZE: usize = 11;
    pub const MAX_DATA_SIZE: usize = 238;

    pub fn try_decode(data: &[u8]) -> Result<Self, ProtocolError> {
        if data.len() < Self::HEADER_SIZE {
            return Err(ProtocolError::BufferTooShort);
        }

        let frame_ctrl = data[0];
        let type_val = frame_ctrl & 0x0F;
        let version = (frame_ctrl >> 4) & 0x0F;

        if version != 0x00 {
            return Err(ProtocolError::InvalidProtocolVersion);
        }

        let frame_type = FrameType::try_from(type_val)?;
        if frame_type == FrameType::Beacon {
            return Err(ProtocolError::InvalidFrameType);
        }

        let src_addr = u32::from_le_bytes(data[1..5].try_into().unwrap());
        let dest_addr = u32::from_le_bytes(data[5..9].try_into().unwrap());
        let flags = data[9];
        let data_len = data[10] as usize;

        if data_len > Self::MAX_DATA_SIZE {
            return Err(ProtocolError::DataTooLarge);
        }

        let expected_len = Self::HEADER_SIZE + data_len;
        if data.len() < expected_len {
            return Err(ProtocolError::BufferTooShort);
        }

        let payload = data[Self::HEADER_SIZE..expected_len].to_vec();

        Ok(Self {
            frame_type,
            src_addr,
            dest_addr,
            flags,
            data: payload,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::HEADER_SIZE + self.data.len());

        buf.push((self.frame_type as u8) | (0x00 << 4));
        buf.extend_from_slice(&self.src_addr.to_le_bytes());
        buf.extend_from_slice(&self.dest_addr.to_le_bytes());
        buf.push(self.flags);
        buf.push(self.data.len() as u8);
        buf.extend_from_slice(&self.data);

        buf
    }
}

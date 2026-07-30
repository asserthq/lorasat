use super::error::ProtocolError;
use super::frame_type::FrameType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beacon {
    pub sat_addr: u32,
    pub interval_sec: u16,
    pub timestamp: u32,
}

impl Beacon {
    pub const MIN_SIZE: usize = 11;

    pub fn try_decode(data: &[u8]) -> Result<Self, ProtocolError> {
        if data.len() < Self::MIN_SIZE {
            return Err(ProtocolError::BufferTooShort);
        }

        let frame_ctrl = data[0];
        let frame_type = frame_ctrl & 0x0F;
        let version = (frame_ctrl >> 4) & 0x0F;

        if frame_type != FrameType::Beacon as u8 {
            return Err(ProtocolError::InvalidFrameType);
        }
        if version != 0x00 {
            return Err(ProtocolError::InvalidProtocolVersion);
        }

        Ok(Self {
            sat_addr: u32::from_le_bytes(data[1..5].try_into().unwrap()),
            interval_sec: u16::from_le_bytes(data[5..7].try_into().unwrap()),
            timestamp: u32::from_le_bytes(data[7..11].try_into().unwrap()),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::MIN_SIZE);
        buf.push(FrameType::Beacon as u8);
        buf.extend_from_slice(&self.sat_addr.to_le_bytes());
        buf.extend_from_slice(&self.interval_sec.to_le_bytes());
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        buf
    }
}

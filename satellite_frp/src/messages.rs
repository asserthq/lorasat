use crate::error::FrpError;

pub const PROTOCOL_ID: u8 = 0x01;
pub const TYPE_START: u8 = 0x01;
pub const TYPE_DATA: u8 = 0x02;
pub const TYPE_ACK: u8 = 0x03;
pub const TYPE_CHECK: u8 = 0x04;

fn make_control_byte(msg_type: u8) -> u8 {
    (PROTOCOL_ID << 4) | (msg_type & 0x0F)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrpStart {
    pub session_id: u16,
    pub total_chunks: u16,
    pub chunk_size: u16,
    pub message_length: u32,
    pub checksum: u32,
}

impl FrpStart {
    pub const SIZE: usize = 15;

    pub fn parse(data: &[u8]) -> Result<Self, FrpError> {
        if data.len() < Self::SIZE {
            return Err(FrpError::BufferTooShort);
        }
        if data[0] != make_control_byte(TYPE_START) {
            return Err(FrpError::InvalidControlByte);
        }

        Ok(Self {
            session_id: u16::from_le_bytes(data[1..3].try_into().unwrap()),
            total_chunks: u16::from_le_bytes(data[3..5].try_into().unwrap()),
            chunk_size: u16::from_le_bytes(data[5..7].try_into().unwrap()),
            message_length: u32::from_le_bytes(data[7..11].try_into().unwrap()),
            checksum: u32::from_le_bytes(data[11..15].try_into().unwrap()),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.push(make_control_byte(TYPE_START));
        buf.extend_from_slice(&self.session_id.to_le_bytes());
        buf.extend_from_slice(&self.total_chunks.to_le_bytes());
        buf.extend_from_slice(&self.chunk_size.to_le_bytes());
        buf.extend_from_slice(&self.message_length.to_le_bytes());
        buf.extend_from_slice(&self.checksum.to_le_bytes());
        buf
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrpData {
    pub session_id: u16,
    pub chunk_index: u16,
    pub flags: u8,
    pub payload: Vec<u8>,
}

impl FrpData {
    pub const HEADER_SIZE: usize = 6;

    pub fn parse(data: &[u8]) -> Result<Self, FrpError> {
        if data.len() < Self::HEADER_SIZE {
            return Err(FrpError::BufferTooShort);
        }
        if data[0] != make_control_byte(TYPE_DATA) {
            return Err(FrpError::InvalidControlByte);
        }

        Ok(Self {
            session_id: u16::from_le_bytes(data[1..3].try_into().unwrap()),
            chunk_index: u16::from_le_bytes(data[3..5].try_into().unwrap()),
            flags: data[5],
            payload: data[6..].to_vec(),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::HEADER_SIZE + self.payload.len());
        buf.push(make_control_byte(TYPE_DATA));
        buf.extend_from_slice(&self.session_id.to_le_bytes());
        buf.extend_from_slice(&self.chunk_index.to_le_bytes());
        buf.push(self.flags);
        buf.extend_from_slice(&self.payload);
        buf
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrpAck {
    pub session_id: u16,
    pub chunk_index: u16,
    pub status: u8,
}

impl FrpAck {
    pub const SIZE: usize = 6;

    pub fn parse(data: &[u8]) -> Result<Self, FrpError> {
        if data.len() < Self::SIZE {
            return Err(FrpError::BufferTooShort);
        }
        if data[0] != make_control_byte(TYPE_ACK) {
            return Err(FrpError::InvalidControlByte);
        }

        Ok(Self {
            session_id: u16::from_le_bytes(data[1..3].try_into().unwrap()),
            chunk_index: u16::from_le_bytes(data[3..5].try_into().unwrap()),
            status: data[5],
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::SIZE);
        buf.push(make_control_byte(TYPE_ACK));
        buf.extend_from_slice(&self.session_id.to_le_bytes());
        buf.extend_from_slice(&self.chunk_index.to_le_bytes());
        buf.push(self.status);
        buf
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrpCheck {
    pub session_id: u16,
    pub total_chunks: u16,
    pub flags: u8,
    pub ack_bitmap: Vec<u8>,
}

impl FrpCheck {
    pub const HEADER_SIZE: usize = 6;

    pub fn parse(data: &[u8]) -> Result<Self, FrpError> {
        if data.len() < Self::HEADER_SIZE {
            return Err(FrpError::BufferTooShort);
        }
        if data[0] != make_control_byte(TYPE_CHECK) {
            return Err(FrpError::InvalidControlByte);
        }

        Ok(Self {
            session_id: u16::from_le_bytes(data[1..3].try_into().unwrap()),
            total_chunks: u16::from_le_bytes(data[3..5].try_into().unwrap()),
            flags: data[5],
            ack_bitmap: data[6..].to_vec(),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::HEADER_SIZE + self.ack_bitmap.len());
        buf.push(make_control_byte(TYPE_CHECK));
        buf.extend_from_slice(&self.session_id.to_le_bytes());
        buf.extend_from_slice(&self.total_chunks.to_le_bytes());
        buf.push(self.flags);
        buf.extend_from_slice(&self.ack_bitmap);
        buf
    }
}

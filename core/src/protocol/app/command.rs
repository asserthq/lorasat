use super::error::ScpError;
use super::status::StatusByte;

pub const PROTOCOL_ID: u8 = 0x02;
pub const TYPE_CMD: u8 = 0x01;
pub const TYPE_ACK_RCV: u8 = 0x02;
pub const TYPE_ACK_EXEC: u8 = 0x03;

fn make_control_byte(msg_type: u8) -> u8 {
    (PROTOCOL_ID << 4) | (msg_type & 0x0F)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command<const PARAM_LEN: usize> {
    pub correlation_id: u16,
    pub subsystem_id: u8,
    pub command_code: u16,
    pub parameters: [u8; PARAM_LEN],
}

impl<const PARAM_LEN: usize> Command<PARAM_LEN> {
    pub const HEADER_SIZE: usize = 6;
    pub const ENCODED_SIZE: usize = Self::HEADER_SIZE + PARAM_LEN;

    pub fn try_decode(data: &[u8]) -> Result<Self, ScpError> {
        if data.len() < Self::HEADER_SIZE + PARAM_LEN {
            return Err(ScpError::BufferTooShort);
        }
        if data[0] != make_control_byte(TYPE_CMD) {
            return Err(ScpError::InvalidControlByte);
        }

        let mut parameters = [0u8; PARAM_LEN];
        parameters.copy_from_slice(&data[6..6 + PARAM_LEN]);

        Ok(Self {
            correlation_id: u16::from_le_bytes([data[1], data[2]]),
            subsystem_id: data[3],
            command_code: u16::from_le_bytes([data[4], data[5]]),
            parameters,
        })
    }

    pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
        let mut buf = [0u8; Self::ENCODED_SIZE];

        buf[0] = make_control_byte(TYPE_CMD);
        buf[1..3].copy_from_slice(&self.correlation_id.to_le_bytes());
        buf[3] = self.subsystem_id;
        buf[4..6].copy_from_slice(&self.command_code.to_le_bytes());
        buf[6..].copy_from_slice(&self.parameters);

        buf
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ack<const DATA_LEN: usize> {
    pub msg_type: u8,
    pub correlation_id: u16,
    pub subsystem_id: u8,
    pub command_code: u16,
    pub status: StatusByte,
    pub subsystem_data: [u8; DATA_LEN],
}

impl<const DATA_LEN: usize> Ack<DATA_LEN> {
    pub const HEADER_SIZE: usize = 7;
    pub const ENCODED_SIZE: usize = Self::HEADER_SIZE + DATA_LEN;

    pub fn try_decode(data: &[u8]) -> Result<Self, ScpError> {
        if data.len() < Self::HEADER_SIZE + DATA_LEN {
            return Err(ScpError::BufferTooShort);
        }

        let ctrl = data[0];
        if ctrl != make_control_byte(TYPE_ACK_RCV) && ctrl != make_control_byte(TYPE_ACK_EXEC) {
            return Err(ScpError::InvalidControlByte);
        }

        let mut subsystem_data = [0u8; DATA_LEN];
        subsystem_data.copy_from_slice(&data[7..7 + DATA_LEN]);

        Ok(Self {
            msg_type: ctrl & 0x0F,
            correlation_id: u16::from_le_bytes([data[1], data[2]]),
            subsystem_id: data[3],
            command_code: u16::from_le_bytes([data[4], data[5]]),
            status: StatusByte(data[6]),
            subsystem_data,
        })
    }

    pub fn encode(&self) -> [u8; Self::ENCODED_SIZE] {
        let mut buf = [0u8; Self::ENCODED_SIZE];

        buf[0] = make_control_byte(self.msg_type);
        buf[1..3].copy_from_slice(&self.correlation_id.to_le_bytes());
        buf[3] = self.subsystem_id;
        buf[4..6].copy_from_slice(&self.command_code.to_le_bytes());
        buf[6] = self.status.0;
        buf[7..].copy_from_slice(&self.subsystem_data);

        buf
    }
}

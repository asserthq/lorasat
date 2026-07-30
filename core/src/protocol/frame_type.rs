use super::error::ProtocolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType {
    Beacon = 0x01,
    DataAsp = 0x02,
    CommandAsp = 0x03,
    SatelliteData = 0x04,
    GroundCommand = 0x05,
}

impl TryFrom<u8> for FrameType {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(FrameType::Beacon),
            0x02 => Ok(FrameType::DataAsp),
            0x03 => Ok(FrameType::CommandAsp),
            0x04 => Ok(FrameType::SatelliteData),
            0x05 => Ok(FrameType::GroundCommand),
            _ => Err(ProtocolError::InvalidFrameType),
        }
    }
}

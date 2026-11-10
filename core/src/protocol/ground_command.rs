use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, defmt::Format)]
pub enum GroundCommand {
    RequestSatTelemetry,
    RequestClientData,
    ChangeBeaconInterval(u32),
    SetTime(u64),
    Ping,
}

impl GroundCommand {
    /// Serializes command into caller-provided buffer.
    pub fn encode<'a>(&self, buffer: &'a mut [u8]) -> Result<&'a [u8], postcard::Error> {
        postcard::to_slice(self, buffer).map(|encoded| &*encoded)
    }

    /// Deserializes command from complete application payload.
    pub fn decode(payload: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(payload)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, defmt::Format)]
pub enum GroundCommandResponse {
    Received,
    Executed,
    Failed,
}

impl GroundCommandResponse {
    /// Serializes response into caller-provided buffer.
    pub fn encode<'a>(&self, buffer: &'a mut [u8]) -> Result<&'a [u8], postcard::Error> {
        postcard::to_slice(self, buffer).map(|encoded| &*encoded)
    }

    /// Deserializes response from complete application payload.
    pub fn decode(payload: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_round_trip() {
        let commands = [
            GroundCommand::RequestSatTelemetry,
            GroundCommand::RequestClientData,
            GroundCommand::ChangeBeaconInterval(30),
            GroundCommand::SetTime(123_456),
            GroundCommand::Ping,
        ];

        for expected in commands {
            let mut buffer = [0_u8; 16];
            let encoded = expected.encode(&mut buffer).unwrap();
            let decoded = GroundCommand::decode(encoded).unwrap();

            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn response_round_trip() {
        let responses = [
            GroundCommandResponse::Received,
            GroundCommandResponse::Executed,
            GroundCommandResponse::Failed,
        ];

        for expected in responses {
            let mut buffer = [0_u8; 8];
            let encoded = expected.encode(&mut buffer).unwrap();
            let decoded = GroundCommandResponse::decode(encoded).unwrap();

            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn command_encode_rejects_small_buffer() {
        let mut buffer = [];

        assert!(GroundCommand::Ping.encode(&mut buffer).is_err());
    }
}

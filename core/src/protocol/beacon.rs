use serde::{Deserialize, Serialize};

use crate::net::address::Address;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, defmt::Format)]
pub struct Beacon {
    pub sat_addr: Address,
    pub interval_sec: u16,
    pub timestamp: u64,
}

impl Beacon {
    /// Serializes beacon into caller-provided buffer.
    pub fn encode<'a>(&self, buffer: &'a mut [u8]) -> Result<&'a [u8], postcard::Error> {
        postcard::to_slice(self, buffer).map(|encoded| &*encoded)
    }

    /// Deserializes beacon from complete application payload.
    pub fn decode(payload: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let expected = Beacon {
            sat_addr: Address::new(42).unwrap(),
            interval_sec: 30,
            timestamp: 123_456,
        };
        let mut buffer = [0_u8; 32];

        let encoded = expected.encode(&mut buffer).unwrap();
        let decoded = Beacon::decode(encoded).unwrap();

        assert_eq!(decoded, expected);
    }

    #[test]
    fn encode_rejects_small_buffer() {
        let beacon = Beacon {
            sat_addr: Address::new(Address::MAX_VALUE).unwrap(),
            interval_sec: u16::MAX,
            timestamp: u64::MAX,
        };
        let mut buffer = [0_u8; 1];

        assert!(beacon.encode(&mut buffer).is_err());
    }
}

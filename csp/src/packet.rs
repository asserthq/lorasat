use crate::{Error, Flags, HEADER_SIZE, Header, wire};

pub const DEFAULT_MTU: usize = 255;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Packet<const MTU: usize> {
    header: Header,
    payload: [u8; MTU],
    payload_len: usize,
}

impl<const MTU: usize> Packet<MTU> {
    pub fn new(header: Header, payload: &[u8]) -> Result<Self, Error> {
        if MTU < HEADER_SIZE {
            return Err(Error::MtuTooSmall {
                minimum: HEADER_SIZE,
                actual: MTU,
            });
        }
        if header.flags() != Flags::NONE && header.flags() != Flags::RDP {
            return Err(Error::UnsupportedFlags(header.flags().bits()));
        }

        let packet_len = HEADER_SIZE + payload.len();
        if packet_len > MTU {
            return Err(Error::PacketTooLarge {
                maximum: MTU,
                actual: packet_len,
            });
        }

        let mut owned_payload = [0; MTU];
        owned_payload[..payload.len()].copy_from_slice(payload);

        Ok(Self {
            header,
            payload: owned_payload,
            payload_len: payload.len(),
        })
    }

    pub fn encode(&self, output: &mut [u8]) -> Result<usize, Error> {
        wire::encode_packet(self, output)
    }

    pub fn decode(input: &[u8]) -> Result<Self, Error> {
        wire::decode_packet(input)
    }

    pub const fn header(&self) -> Header {
        self.header
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len]
    }

    pub(crate) fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.payload[..self.payload_len]
    }

    pub const fn encoded_len(&self) -> usize {
        HEADER_SIZE + self.payload_len
    }
}

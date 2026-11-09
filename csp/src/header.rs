use crate::{Error, wire};

pub const HEADER_SIZE: usize = 6;
pub const MAX_NODE_ID: u16 = 0x3FFF;
pub const MAX_PORT: u8 = 0x3F;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Priority {
    Critical = 0,
    High = 1,
    Normal = 2,
    Low = 3,
}

impl Priority {
    pub(crate) const fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Critical,
            1 => Self::High,
            2 => Self::Normal,
            3 => Self::Low,
            _ => unreachable!(),
        }
    }

    pub(crate) const fn bits(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Flags(u8);

impl Flags {
    pub const NONE: Self = Self(0);
    pub const RESERVED: Self = Self(0x20);
    pub const FRAGMENTATION: Self = Self(0x10);
    pub const HMAC: Self = Self(0x08);
    pub const RDP: Self = Self(0x02);
    pub const CRC32: Self = Self(0x01);

    pub const fn from_bits(bits: u8) -> Result<Self, Error> {
        if bits <= 0x3F {
            Ok(Self(bits))
        } else {
            Err(Error::InvalidFlags(bits))
        }
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    priority: Priority,
    source: u16,
    destination: u16,
    source_port: u8,
    destination_port: u8,
    flags: Flags,
}

impl Header {
    /// Creates a CSP v2 header using source-first argument order.
    pub const fn new(
        priority: Priority,
        source: u16,
        destination: u16,
        source_port: u8,
        destination_port: u8,
        flags: Flags,
    ) -> Result<Self, Error> {
        if source > MAX_NODE_ID {
            return Err(Error::InvalidSource(source));
        }
        if destination > MAX_NODE_ID {
            return Err(Error::InvalidDestination(destination));
        }
        if source_port > MAX_PORT {
            return Err(Error::InvalidSourcePort(source_port));
        }
        if destination_port > MAX_PORT {
            return Err(Error::InvalidDestinationPort(destination_port));
        }

        Ok(Self {
            priority,
            source,
            destination,
            source_port,
            destination_port,
            flags,
        })
    }

    pub fn encode(self, output: &mut [u8]) -> Result<usize, Error> {
        wire::encode_header(self, output)
    }

    pub fn decode(input: &[u8]) -> Result<Self, Error> {
        wire::decode_header(input)
    }

    pub const fn priority(self) -> Priority {
        self.priority
    }

    pub const fn source(self) -> u16 {
        self.source
    }

    pub const fn destination(self) -> u16 {
        self.destination
    }

    pub const fn source_port(self) -> u8 {
        self.source_port
    }

    pub const fn destination_port(self) -> u8 {
        self.destination_port
    }

    pub const fn flags(self) -> Flags {
        self.flags
    }
}

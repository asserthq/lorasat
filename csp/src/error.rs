use core::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidSource(u16),
    InvalidDestination(u16),
    InvalidSourcePort(u8),
    InvalidDestinationPort(u8),
    InvalidFlags(u8),
    UnsupportedFlags(u8),
    InputTooShort { required: usize, actual: usize },
    OutputTooShort { required: usize, actual: usize },
    PacketTooLarge { maximum: usize, actual: usize },
    MtuTooSmall { minimum: usize, actual: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSource(value) => {
                write!(formatter, "source address {value} exceeds 14 bits")
            }
            Self::InvalidDestination(value) => {
                write!(formatter, "destination address {value} exceeds 14 bits")
            }
            Self::InvalidSourcePort(value) => {
                write!(formatter, "source port {value} exceeds 6 bits")
            }
            Self::InvalidDestinationPort(value) => {
                write!(formatter, "destination port {value} exceeds 6 bits")
            }
            Self::InvalidFlags(value) => write!(formatter, "flags {value:#04x} exceed 6 bits"),
            Self::UnsupportedFlags(value) => {
                write!(formatter, "flags {value:#04x} are not supported")
            }
            Self::InputTooShort { required, actual } => {
                write!(formatter, "input needs {required} bytes, got {actual}")
            }
            Self::OutputTooShort { required, actual } => {
                write!(formatter, "output needs {required} bytes, got {actual}")
            }
            Self::PacketTooLarge { maximum, actual } => {
                write!(formatter, "packet is {actual} bytes, maximum is {maximum}")
            }
            Self::MtuTooSmall { minimum, actual } => {
                write!(formatter, "MTU is {actual} bytes, minimum is {minimum}")
            }
        }
    }
}

use std::io;

use sat_core::comm::Endpoint;

#[derive(Debug)]
pub enum MockCspError {
    Io(io::Error),
    InvalidEndpoint(Endpoint),
    BufferTooSmall { required: usize, available: usize },
    MessageTooLarge { length: usize, maximum: usize },
    Closed,
}

impl From<io::Error> for MockCspError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

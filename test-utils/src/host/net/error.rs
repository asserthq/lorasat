use std::io;

use sat_core::net::Endpoint;

#[derive(Debug)]
pub enum MockNetError {
    Io(io::Error),
    InvalidEndpoint(Endpoint),
    BufferTooSmall { required: usize, available: usize },
    MessageTooLarge { length: usize, maximum: usize },
    Closed,
}

impl From<io::Error> for MockNetError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

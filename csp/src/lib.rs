#![no_std]
#![forbid(unsafe_code)]

//! Owned CSP v2 packets for `no_std` systems.
//!
//! Byte serialization is isolated in the private `wire` module. Current packet
//! support intentionally rejects optional protocol flags until their trailers
//! and service headers are implemented.

mod error;
mod header;
mod packet;
mod wire;

pub use error::Error;
pub use header::{Flags, HEADER_SIZE, Header, MAX_NODE_ID, MAX_PORT, Priority};
pub use packet::{DEFAULT_MTU, Packet};

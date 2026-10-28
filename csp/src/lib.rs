#![no_std]
#![forbid(unsafe_code)]

//! Owned CSP v2 packets, unreliable datagrams, and an initial RDP profile for `no_std` systems.
//!
//! Byte serialization is isolated in the private `wire` module. [`Runner`]
//! owns one packet [`Link`], while application code uses bounded-queue
//! [`Socket`] handles. No executor is selected by this crate.
//!
//! RDP supports one connection and one confirmed message per handle. Other
//! optional protocol flags remain rejected until their trailers are implemented.

mod error;
mod header;
mod link;
mod packet;
mod rdp;
mod runner;
mod socket;
mod wire;

pub use error::Error;
pub use header::{Flags, HEADER_SIZE, Header, MAX_NODE_ID, MAX_PORT, Priority};
pub use link::Link;
pub use packet::{DEFAULT_MTU, Packet};
pub use rdp::{
    RDP_CLIENT_PORT, RDP_SYN_OPTIONS_SIZE, RDP_TRAILER_SIZE, RdpClient, RdpConfig, RdpConnection,
    RdpError, RdpListener, RdpMessage, RdpResources,
};
pub use runner::{Runner, RunnerBuilder, RunnerError, RunnerStats, StackResources};
pub use socket::{BindError, Datagram, SendError, Socket};

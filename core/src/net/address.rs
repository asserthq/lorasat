pub use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, defmt::Format, Default)]
pub struct Address(pub u32);

pub const BROADCAST_ADDRESS: Address = Address(0xFFFFFFFF);

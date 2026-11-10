pub use serde::{Deserialize, Serialize};

#[repr(transparent)]
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    defmt::Format,
)]
pub struct Address(u16);

impl Address {
    pub const MAX_VALUE: u16 = 1 << 14;
    pub const BROADCAST: Self = Self(Self::MAX_VALUE);

    pub const fn new(value: u16) -> Option<Self> {
        if value <= Self::MAX_VALUE {
            Some(Self(value))
        } else {
            None
        }
    }

    pub const fn new_unicast(value: u16) -> Option<Self> {
        if value < Self::MAX_VALUE {
            Some(Self(value))
        } else {
            None
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const fn is_broadcast(self) -> bool {
        self.0 == Self::MAX_VALUE
    }

    pub const fn is_unicast(self) -> bool {
        !self.is_broadcast()
    }
}

impl From<Address> for u16 {
    fn from(address: Address) -> Self {
        address.get()
    }
}

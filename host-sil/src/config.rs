use sat_core::net::Address;

pub const MTU: usize = 255;
pub const SAT_ADDR: Address = Address::new_unicast(1).unwrap();
pub const GS_ADDR: Address = Address::new_unicast(2).unwrap();
pub const CLIENT_ADDR: Address = Address::new_unicast(3).unwrap();

pub const BEACON_PORT: u8 = 10;
pub const CLIENT_DATA_PORT: u8 = 11;
pub const CMD_PORT: u8 = 20;

pub const BEACON_INTERVAL_SECS: u16 = 10;
pub const DUMP_COMMAND: &[u8] = b"dump";

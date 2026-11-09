pub const MTU: usize = 255;
pub const SAT_ADDR: Address = Address(1);
pub const GS_ADDR: Address = Address(2);
pub const CLIENT_ADDR: Address = Address(3);

pub const BEACON_PORT: u8 = 10;
pub const CLIENT_DATA_PORT: u8 = 11;
pub const CMD_PORT: u8 = 20;

pub const BEACON_INTERVAL_SECS: u16 = 10;
pub const DUMP_COMMAND: &[u8] = b"dump";

const BEACON_VERSION: u8 = 1;
const BEACON_SIZE: usize = 13;
const DUMP_VERSION: u8 = 1;
const RDP_APPLICATION_MAX: usize = MTU - sat_csp::HEADER_SIZE - sat_csp::RDP_TRAILER_SIZE;

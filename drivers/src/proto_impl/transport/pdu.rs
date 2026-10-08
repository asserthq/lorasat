use heapless::Vec;
use serde::{Deserialize, Serialize};

/// Максимальный размер payload (байт).
pub const MTU: usize = 240;

/// Максимальное число чанков в одном send().
pub const MAX_CHUNKS: usize = 32;

pub struct Packet {
    pub sport: u8,
    pub dport: u8,
    pub pdu: Pdu,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Pdu<'a> {
    Connect,
    Accept,
    Data {
        seq: u32,
        first: bool,
        last: bool,
        payload: &'a [u8],
    },
    Ack {
        cumulative_seq: u32,
        bitmap: u16,
    },
}

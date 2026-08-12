use heapless::Vec;
use serde::{Deserialize, Serialize};

pub const MAX_CLIENT_DATA: usize = 1024 * 32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientData {
    pub data: Vec<u8, MAX_CLIENT_DATA>,
}

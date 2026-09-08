use serde::{Deserialize, Serialize};

pub const MAX_CLIENT_DATA: usize = 255;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct ClientData<'a> {
    pub data: &'a [u8],
}

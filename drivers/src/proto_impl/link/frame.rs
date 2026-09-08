use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct Frame<'a> {
    pub src: u32,
    pub dst: u32,
    pub payload: &'a [u8],
}

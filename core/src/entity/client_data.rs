use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, defmt::Format)]
pub struct ClientData<'a> {
    #[serde(borrow)]
    pub data: &'a [u8],
}

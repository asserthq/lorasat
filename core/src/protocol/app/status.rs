#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusByte(pub u8);

impl StatusByte {
    pub fn new(proto_status: u8, sub_status: u8) -> Self {
        Self(((proto_status & 0x0F) << 4) | (sub_status & 0x0F))
    }

    pub fn proto_status(&self) -> u8 {
        (self.0 >> 4) & 0x0F
    }

    pub fn sub_status(&self) -> u8 {
        self.0 & 0x0F
    }
}

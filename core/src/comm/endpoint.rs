use crate::comm::Address;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, defmt::Format)]
pub struct Endpoint {
    addr: Address,
    port: u8,
}

impl Endpoint {
    pub const fn new(addr: Address, port: u8) -> Self {
        Self { addr, port }
    }

    pub const fn port(&self) -> u8 {
        self.port
    }

    pub const fn addr(&self) -> Address {
        self.addr
    }
}

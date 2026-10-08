use crate::comm::Address;

#[derive(Debug, Clone, Default)]
pub struct Endpoint {
    addr: Address,
    port: u8,
}

impl Endpoint {
    pub fn new(addr: Address, port: u8) -> Self {
        Self { addr, port }
    }

    pub fn port(&self) -> u8 {
        self.port
    }

    pub fn addr(&self) -> Address {
        self.addr
    }
}

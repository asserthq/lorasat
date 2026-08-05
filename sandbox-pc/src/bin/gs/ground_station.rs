use sat_core::radio::HalfDuplexTransceiver;

pub struct GroundStation<TRX: HalfDuplexTransceiver> {
    trx: TRX,
}

impl<TRX: HalfDuplexTransceiver> GroundStation<TRX> {
    pub fn new(trx: TRX) -> Self {
        Self { trx }
    }

    pub async fn run(&mut self) {
        loop {}
    }

    async fn send_command(&mut self) {}
}

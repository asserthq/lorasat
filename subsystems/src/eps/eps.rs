use defmt_or_log::info;
use embassy_time::{Duration, Timer};
use sat_core::eps::{EpsCommand, EpsEvent};

use crate::link::WorkerHandle;

type EpsHandle<'a> = WorkerHandle<'a, EpsCommand, EpsEvent>;

pub struct EpsSystem<'a> {
    handle: EpsHandle<'a>,
}

impl<'a> EpsSystem<'a> {
    pub fn new(handle: EpsHandle<'a>) -> Self {
        Self { handle }
    }

    pub async fn run(&mut self) {
        info!("EPS started");
        loop {
            Timer::after(Duration::from_secs(5)).await;
            info!("EPS working ...");
            self.send_event(EpsEvent::TestEvent("mega work event"))
                .await;
        }
    }

    async fn send_event(&mut self, e: EpsEvent) {
        self.handle.event_tx.send(e).await;
    }
}

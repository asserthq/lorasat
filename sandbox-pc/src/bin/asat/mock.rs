use core::time::Duration;

use sat_core::adcs::coils::{CoilLevelsVec, Coils};
use sat_core::adcs::mag::{BVec, Mag};
use sat_core::time::Delay;

pub struct MagMock {}

impl Mag for MagMock {
    async fn read(&self) -> BVec {
        println!("read MAG");
        BVec::default()
    }
}

pub struct CoilsMock {}

impl Coils for CoilsMock {
    async fn apply_levels(&mut self, _levels: CoilLevelsVec) {
        println!("apply COILS");
    }
}

pub struct DelayMock {}

impl Delay for DelayMock {
    async fn delay(&mut self, dur: Duration) {
        println!("delay 200");
        tokio::time::sleep(dur).await;
    }
}

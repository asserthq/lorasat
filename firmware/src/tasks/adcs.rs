use core::time::Duration;

use embassy_time::Timer;
use sat_core::{
    adcs::{AdcsCommand, AdcsEvent},
    time::Delay,
};
use sat_subsystems::{adcs::AdcsSystem, link::WorkerHandle};

use crate::devices::adcs::{DisabledCoils, Magnetometer};

#[embassy_executor::task]
pub(crate) async fn run(
    magnetometer: Magnetometer,
    coils: DisabledCoils,
    handle: WorkerHandle<'static, AdcsCommand, AdcsEvent>,
) {
    let mut system = AdcsSystem::new(magnetometer, coils, EmbassyDelay, handle);

    match system.run().await {
        Ok(()) => panic!("ADCS task returned unexpectedly"),
        Err(_) => panic!("ADCS task failed"),
    }
}

struct EmbassyDelay;

impl Delay for EmbassyDelay {
    async fn delay(&mut self, duration: Duration) {
        Timer::after(embassy_time::Duration::from_micros(
            duration.as_micros() as u64
        ))
        .await;
    }
}

use sat_core::eps::{EpsCommand, EpsEvent};
use sat_subsystems::{eps::EpsSystem, link::WorkerHandle};

#[embassy_executor::task]
pub(crate) async fn run(handle: WorkerHandle<'static, EpsCommand, EpsEvent>) {
    let mut system = EpsSystem::new(handle);
    system.run().await;
    panic!("EPS task returned unexpectedly")
}

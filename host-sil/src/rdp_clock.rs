use core::time::Duration;

use sat_csp::RdpResources;
use tokio::time::{Instant, MissedTickBehavior, interval};

pub async fn run<const MTU: usize>(resources: &RdpResources<MTU>) -> ! {
    let epoch = Instant::now();
    let mut ticks = interval(Duration::from_millis(10));
    ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticks.tick().await;
        resources.update_time(epoch.elapsed().as_millis() as u32);
    }
}

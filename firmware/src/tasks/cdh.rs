use embassy_futures::select::select3;
use embassy_time::{Duration, Instant, Timer};
use sat_csp::{CspInterface, RdpResources};
use sat_drivers::storage::RamStorage;
use sat_subsystems::cdh::{CdhConfig, CdhSystem};

use crate::{
    channels,
    config::{
        cdh::BEACON_INTERVAL_SECS,
        network::{BEACON_PORT, MTU, SATELLITE_ADDRESS},
        storage::{DEPTH as STORAGE_DEPTH, ENTRY_SIZE as STORAGE_ENTRY_SIZE},
    },
    network::CspStack,
    resources::ProtocolStackResources,
};

pub(crate) async fn run<I>(interface: I) -> !
where
    I: CspInterface,
{
    let resources = ProtocolStackResources::new();
    let CspStack {
        beacon_socket,
        command_listener,
        mut runner,
    } = CspStack::bind(interface, &resources).expect("CSP stack configuration is valid");

    let subsystem_links = channels::bundle();
    let _system_handles = channels::system_handles();
    let mut cdh = CdhSystem::new(
        CdhConfig {
            beacon_interval: Duration::from_secs(u64::from(BEACON_INTERVAL_SECS)),
            satellite_address: SATELLITE_ADDRESS,
            beacon_port: BEACON_PORT,
        },
        subsystem_links,
        beacon_socket,
        command_listener,
        RamStorage::<STORAGE_ENTRY_SIZE, STORAGE_DEPTH>::new(),
        RamStorage::<STORAGE_ENTRY_SIZE, STORAGE_DEPTH>::new(),
    );

    let _ = select3(runner.run(), cdh.run(), run_rdp_clock(&resources.rdp)).await;

    panic!("CDH task stopped")
}

async fn run_rdp_clock(resources: &RdpResources<MTU>) -> ! {
    loop {
        resources.update_time(Instant::now().as_millis() as u32);
        Timer::after_millis(10).await;
    }
}

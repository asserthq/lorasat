mod channels;
mod network;

use embassy_time::Duration;
use host_sil::{
    config::{BEACON_INTERVAL_SECS, BEACON_PORT, SAT_ADDR},
    rdp_clock,
};
use network::{CspRdpResources, CspStack, CspStackResources};
use sat_drivers::storage::RamStorage;
use sat_subsystems::cdh::{CdhConfig, CdhSystem};

const STORAGE_ENTRY_SIZE: usize = 300;
const STORAGE_DEPTH: usize = 16;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let csp_resources = CspStackResources::new();
    let reliable_resources = CspRdpResources::new();
    let CspStack {
        beacon_socket,
        command_listener,
        mut runner,
    } = CspStack::bind(&csp_resources, &reliable_resources).await;

    let subsystems = channels::bundle();
    let _system_handles = channels::system_handles();
    let mut cdh = CdhSystem::new(
        CdhConfig {
            beacon_interval: Duration::from_secs(u64::from(BEACON_INTERVAL_SECS)),
            satellite_address: SAT_ADDR,
            beacon_port: BEACON_PORT,
        },
        subsystems,
        beacon_socket,
        command_listener,
        RamStorage::<STORAGE_ENTRY_SIZE, STORAGE_DEPTH>::new(),
        RamStorage::<STORAGE_ENTRY_SIZE, STORAGE_DEPTH>::new(),
    );

    tokio::select! {
        result = runner.run() => panic!("CSP runner stopped: {result:?}"),
        () = cdh.run() => unreachable!("CDH task stopped"),
        _ = rdp_clock::run(&reliable_resources) => unreachable!("RDP clock stopped"),
    }
}

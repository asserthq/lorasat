mod beacons;
mod commands;
mod network;

use host_sil::rdp_clock;
use network::{CspResources, CspStack, ReliableResources};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let csp_resources = CspResources::new();
    let reliable_resources = ReliableResources::new();
    let CspStack {
        beacon_socket,
        command_client,
        mut runner,
    } = CspStack::bind(&csp_resources, &reliable_resources).await;

    println!("ground: receiving satellite beacons; type commands in this terminal");
    tokio::select! {
        result = runner.run() => panic!("CSP runner stopped: {result:?}"),
        _ = beacons::receive(beacon_socket) => unreachable!("beacon receiver stopped"),
        () = commands::run(command_client) => println!("ground: command input stopped"),
        _ = rdp_clock::run(&reliable_resources) => unreachable!("RDP clock stopped"),
    }
}

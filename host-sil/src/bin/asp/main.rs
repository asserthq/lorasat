mod aloha;
mod application;
mod generator;
mod network;
mod random;

use network::{CspResources, CspStack};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let random = random::Random::from_host().expect("invalid ASP random seed configuration");
    let resources = CspResources::new();
    let CspStack {
        beacon_socket,
        data_socket,
        mut runner,
    } = CspStack::bind(&resources).await;

    tokio::select! {
        result = runner.run() => panic!("ASP CSP runner stopped: {result:?}"),
        _ = application::run(beacon_socket, data_socket, random) => unreachable!("ASP application stopped"),
    }
}

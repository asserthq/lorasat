use log::*;
use sandbox_pc::mock::TransportMockTCP;
use sat_core::comm::address::Address;
use sat_subsystems::comm::{CommConfig, CommSystem};

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    // логи подсистем (defmt-or-log -> log) в stderr; уровень из RUST_LOG, дефолт info
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("logger built");

    let stack = TransportMockTCP::from_addr(Address(0x11)).await.unwrap();

    let comm_config = CommConfig {
        sat_addr: Address(0x11),
        beacon_interval_sec: 10,
    };

    let mut comm = CommSystem::new(stack, comm_config);

    info!("comm system created");

    comm.run().await;

    Ok(())
}

use sandbox_pc::mock::TransportMockTCP;
use sat_core::comm::address::Address;
use sat_subsystems::comm::{CommConfig, CommSystem};

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let stack = TransportMockTCP::from_addr(Address(0x11)).await.unwrap();

    let comm_config = CommConfig {
        sat_addr: Address(0x11),
        beacon_interval_sec: 10,
    };

    let mut comm = CommSystem::new(stack, comm_config);

    comm.run().await;

    Ok(())
}

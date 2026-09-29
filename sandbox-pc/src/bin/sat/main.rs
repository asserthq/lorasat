use log::*;
#[allow(unused_imports)]
use sandbox_pc::mock::{PhyRadioUDP, TransportMockTCP};
use sat_core::comm::address::Address;
use sat_drivers::proto_impl::{link::LinkImpl, transport::TransportImpl};
use sat_subsystems::comm::{CommConfig, CommSystem};

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    // логи подсистем (defmt-or-log -> log) в stderr; уровень из RUST_LOG, дефолт info
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("logger built");

    let sat_addr = Address(0x11);
    let gs_addr = Address(0x01);

    let phy = PhyRadioUDP::new(sat_addr, &[gs_addr]).await.unwrap();
    let link = LinkImpl::new(sat_addr, phy);
    let transport = TransportImpl::new(link);

    // let stack = TransportMockTCP::new(sat_addr, &[gs_addr]).await.unwrap();

    let comm_config = CommConfig {
        sat_addr,
        beacon_interval_sec: 10,
    };

    let mut comm = CommSystem::new(transport, comm_config);

    info!("comm system created");

    comm.run().await;

    Ok(())
}

mod ground_station;

use ground_station::GroundStation;

use sandbox_pc::udp_ports::{GS_PORT, SAT_PORT_435};
use sandbox_pc::udp_transciever::UdpPhysicalMock;

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let radio435 = UdpPhysicalMock::from_ports(GS_PORT, SAT_PORT_435).await?;
    let mut gs = GroundStation::new(radio435);
    gs.run().await;
    Ok(())
}

mod satellite;

use sandbox_pc::udp_ports::{CLIENT_PORT, GS_PORT, SAT_PORT_435, SAT_PORT_868};
use sandbox_pc::udp_transciever::UdpTransciever;
use satellite::Satellite;

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let radio435 = UdpTransciever::from_ports(SAT_PORT_435, GS_PORT).await?;
    let radio868 = UdpTransciever::from_ports(SAT_PORT_868, CLIENT_PORT).await?;
    let sat = Satellite::new(radio435, radio868);
    sat.run().await;
    Ok(())
}

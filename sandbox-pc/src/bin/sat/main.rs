mod satellite;

use sandbox_pc::udp_transciever::UdpTransciever;
use satellite::Satellite;

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let radio435 = UdpTransciever::from_ports(9001, 9002).await?;
    let radio868 = UdpTransciever::from_ports(9003, 9004).await?;
    let sat = Satellite::new(radio435, radio868);
    sat.run().await;
    Ok(())
}

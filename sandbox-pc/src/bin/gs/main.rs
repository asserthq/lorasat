mod ground_station;

use ground_station::GroundStation;

use sandbox_pc::udp_transciever::UdpTransciever;

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let radio435 = UdpTransciever::from_ports(9002, 9001).await?;
    let mut gs = GroundStation::new(radio435);
    gs.run().await;
    Ok(())
}

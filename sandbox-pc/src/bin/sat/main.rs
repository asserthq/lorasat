mod satellite;

use sandbox_pc::udp_transciever::UdpTransciever;
use satellite::Satellite;

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let radio = UdpTransciever::from_ports(9001, 9002).await?;
    let sat = Satellite::new(radio);
    sat.run().await;
    Ok(())
}

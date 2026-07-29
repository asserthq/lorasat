mod client_device;

use std::io;

use client_device::ClientDevice;
use sandbox_pc::udp_transciever::UdpTransciever;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let radio = UdpTransciever::from_ports(9002, 9001).await?;
    let gnd = ClientDevice::new(radio);
    gnd.run().await;
    Ok(())
}

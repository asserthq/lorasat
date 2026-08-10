mod client_device;
mod telemetry;

use std::io;

use client_device::ClientDevice;
use sandbox_pc::udp_ports::{CLIENT_PORT, SAT_PORT_868};
use sandbox_pc::udp_transciever::UdpTransciever;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let radio = UdpTransciever::from_ports(CLIENT_PORT, SAT_PORT_868).await?;
    let gnd = ClientDevice::new(radio);
    gnd.run().await;
    Ok(())
}

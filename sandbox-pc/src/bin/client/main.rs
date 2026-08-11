mod client_device;
mod telemetry;

use std::io;

use client_device::ClientDevice;
use sandbox_pc::udp_ports::{CLIENT_PORT, SAT_PORT_868};
use sandbox_pc::udp_transciever::UdpPhysicalMock;
use sat_core::proto::data_link::codec::DataLinkCodec;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let radio = UdpPhysicalMock::from_ports(CLIENT_PORT, SAT_PORT_868).await?;
    let link = DataLinkCodec::new(radio);

    let client = ClientDevice::new(link);
    client.run().await;

    Ok(())
}

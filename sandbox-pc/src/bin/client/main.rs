mod client_device;
mod telemetry;

use std::io;

use client_device::ClientDevice;
use sandbox_pc::udp_physical_mock::UdpPhysicalMock;
use sandbox_pc::udp_ports::{CLIENT_PORT, SAT_PORT_868};
use sat_core::proto::data_link::codec::DataLinkCodec;
use sat_core::proto::transport::simple::SimpleTransport;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let radio868 = UdpPhysicalMock::from_ports(CLIENT_PORT, SAT_PORT_868).await?;
    let codec868 = DataLinkCodec::new(radio868);
    let transport = SimpleTransport::new(CLIENT_PORT as u32, codec868);

    let client = ClientDevice::new(transport);
    client.run().await;

    Ok(())
}

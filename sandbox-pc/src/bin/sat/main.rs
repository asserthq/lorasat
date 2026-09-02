mod satellite;

use sandbox_pc::udp_physical_mock::UdpPhysicalMock;
use sandbox_pc::udp_ports::{CLIENT_PORT, GS_PORT, SAT_PORT_435, SAT_PORT_868};
use sat_core::proto::link::codec::DataLinkCodec;
use sat_core::proto::transport::simple::SimpleTransport;
use satellite::Satellite;

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    // Build protocol stacks for both radios.
    // src_addr is set at layer construction, dest_addr flows via messages.

    let radio435 = UdpPhysicalMock::from_ports(SAT_PORT_435, GS_PORT).await?;
    let link435 = DataLinkCodec::new(radio435);
    let transport_gs = SimpleTransport::new(SAT_PORT_435 as u32, link435);

    let radio868 = UdpPhysicalMock::from_ports(SAT_PORT_868, CLIENT_PORT).await?;
    let link868 = DataLinkCodec::new(radio868);
    let transport_client = SimpleTransport::new(SAT_PORT_868 as u32, link868);

    let sat = Satellite::new(transport_gs, transport_client);
    sat.run().await;

    Ok(())
}

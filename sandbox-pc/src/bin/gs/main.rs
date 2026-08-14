mod ground_station;

use ground_station::GroundStation;

use sandbox_pc::udp_physical_mock::UdpPhysicalMock;
use sandbox_pc::udp_ports::{GS_PORT, SAT_PORT_435};
use sat_core::proto::data_link::codec::DataLinkCodec;
//use sat_core::proto::transport::frp::FrpTransport;
use sat_core::proto::transport::simple::SimpleTransport;

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    // Build protocol stack bottom-up for 435 MHz link.
    let radio435 = UdpPhysicalMock::from_ports(GS_PORT, SAT_PORT_435).await?;
    let link435 = DataLinkCodec::new(radio435);
    // src_addr = my address (GS_PORT), chunk_size = 240
    //let transport435 = FrpTransport::new(link435, GS_PORT as u32, 240);
    let transport435 = SimpleTransport::new(GS_PORT as u32, link435);

    let mut gs = GroundStation::new(transport435);
    gs.run().await;

    Ok(())
}

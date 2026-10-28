use examples_std::csp_demo::{
    BEACON_PORT, CLIENT_ADDRESS, CLIENT_DATA_PORT, MTU, RadioBand, SATELLITE_ADDRESS, UdpRadioLink,
};
use sat_csp::{RunnerBuilder, StackResources};
use tokio::time::{Duration, sleep};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let payload = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "opaque client data: temperature=23".to_owned());
    let link = UdpRadioLink::bind(RadioBand::Mhz868, CLIENT_ADDRESS, &[SATELLITE_ADDRESS])
        .await
        .unwrap();
    let stack = StackResources::<MTU, 2, 1, 2>::new();

    let mut builder = RunnerBuilder::new(CLIENT_ADDRESS, link, &stack).unwrap();
    let mut socket = builder.bind(BEACON_PORT).unwrap();
    let mut runner = builder.build();

    let application = async {
        println!("client: waiting for 868 MHz beacon");
        let datagram = socket.recv_from().await;
        let beacon = examples_std::csp_demo::Beacon::decode(datagram.payload()).unwrap();
        println!(
            "client: beacon from CSP {}: time={}, interval={}s",
            beacon.satellite_address, beacon.unix_time_seconds, beacon.interval_seconds
        );

        socket
            .send_to(
                beacon.satellite_address,
                CLIENT_DATA_PORT,
                payload.as_bytes(),
            )
            .await
            .unwrap();
        println!(
            "client: queued {} opaque bytes for satellite; payload={payload:?}",
            payload.len()
        );

        // Datagram send confirms only local queueing. Give Runner time to send it over UDP.
        sleep(Duration::from_millis(100)).await;
    };

    tokio::select! {
        result = runner.run() => panic!("client Runner stopped: {result:?}"),
        () = application => {}
    }
}

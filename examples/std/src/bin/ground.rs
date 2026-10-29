use std::io::Write;

use examples_std::csp_demo::{
    BEACON_PORT, COMMAND_PORT, DUMP_COMMAND, GROUND_ADDRESS, MTU, RadioBand, SATELLITE_ADDRESS,
    UdpRadioInterface, decode_dump,
};
use sat_csp::{RdpConfig, RdpResources, RunnerBuilder, StackResources};
use tokio::time::{Duration, interval};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let interface =
        UdpRadioInterface::bind(RadioBand::Mhz435, GROUND_ADDRESS, &[SATELLITE_ADDRESS])
            .await
            .unwrap();
    let stack = StackResources::<MTU, 4, 1, 4>::new();
    let rdp = RdpResources::<MTU>::new();
    rdp.update_time(0);

    let mut builder = RunnerBuilder::new(GROUND_ADDRESS, interface, &stack).unwrap();
    let mut beacon_socket = builder.bind(BEACON_PORT).unwrap();
    let client = builder.rdp_client(&rdp, RdpConfig::new(0x1000)).unwrap();
    let mut runner = builder.build();

    let application = async {
        println!("ground: waiting for 435 MHz beacon");
        let beacon_datagram = beacon_socket.recv_from().await;
        let beacon = examples_std::csp_demo::Beacon::decode(beacon_datagram.payload()).unwrap();
        println!(
            "ground: beacon from CSP {}: time={}, interval={}s",
            beacon.satellite_address, beacon.unix_time_seconds, beacon.interval_seconds
        );

        print!("ground: press Enter to request subscriber records... ");
        std::io::stdout().flush().unwrap();
        tokio::task::spawn_blocking(|| {
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).unwrap();
        })
        .await
        .unwrap();

        let mut connection = client
            .connect(SATELLITE_ADDRESS, COMMAND_PORT)
            .await
            .unwrap();
        connection.send(DUMP_COMMAND).await.unwrap();
        println!("ground: dump command delivery acknowledged");

        let response = connection.recv().await;
        let dump = decode_dump(response.payload()).unwrap();
        println!(
            "ground: received {} of {} stored records",
            dump.records.len(),
            dump.total_records
        );
        for (index, record) in dump.records.iter().enumerate() {
            println!(
                "  #{index}: source={}:{}, {} bytes, raw={:02X?}, text={:?}",
                record.source,
                record.source_port,
                record.payload.len(),
                record.payload,
                String::from_utf8_lossy(&record.payload)
            );
        }
        if dump.records.len() < dump.total_records {
            println!("ground: some records did not fit in one RDP DATA packet");
        }
    };

    let clock = async {
        let mut now_ms = 0_u32;
        let mut ticks = interval(Duration::from_millis(10));
        loop {
            ticks.tick().await;
            now_ms = now_ms.wrapping_add(10);
            rdp.update_time(now_ms);
        }
    };

    tokio::select! {
        result = runner.run() => panic!("ground Runner stopped: {result:?}"),
        () = clock => unreachable!(),
        () = application => {}
    }
}

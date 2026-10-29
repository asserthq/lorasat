use std::{future::pending, sync::Arc};

use examples_std::csp_demo::{
    BEACON_INTERVAL_SECONDS, BEACON_PORT, CLIENT_ADDRESS, CLIENT_DATA_PORT, COMMAND_PORT,
    DUMP_COMMAND, GROUND_ADDRESS, MTU, OpaqueRecord, RadioBand, SATELLITE_ADDRESS,
    UdpRadioInterface, encode_dump, unix_time_seconds,
};
use sat_csp::{MAX_NODE_ID, RdpConfig, RdpResources, RunnerBuilder, StackResources};
use tokio::{
    sync::Mutex,
    time::{Duration, interval},
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let interface_435 =
        UdpRadioInterface::bind(RadioBand::Mhz435, SATELLITE_ADDRESS, &[GROUND_ADDRESS])
            .await
            .unwrap();
    let interface_868 =
        UdpRadioInterface::bind(RadioBand::Mhz868, SATELLITE_ADDRESS, &[CLIENT_ADDRESS])
            .await
            .unwrap();

    let stack_435 = StackResources::<MTU, 4, 1, 4>::new();
    let stack_868 = StackResources::<MTU, 4, 2, 4>::new();
    let rdp_435 = RdpResources::<MTU>::new();
    rdp_435.update_time(0);

    let mut builder_435 = RunnerBuilder::new(SATELLITE_ADDRESS, interface_435, &stack_435).unwrap();
    let beacon_435 = builder_435.bind(BEACON_PORT).unwrap();
    let command_listener = builder_435
        .listen_rdp(COMMAND_PORT, &rdp_435, RdpConfig::new(0x2000))
        .unwrap();
    let mut runner_435 = builder_435.build();

    let mut builder_868 = RunnerBuilder::new(SATELLITE_ADDRESS, interface_868, &stack_868).unwrap();
    let beacon_868 = builder_868.bind(BEACON_PORT).unwrap();
    let mut client_data = builder_868.bind(CLIENT_DATA_PORT).unwrap();
    let mut runner_868 = builder_868.build();

    let records = Arc::new(Mutex::new(Vec::<OpaqueRecord>::new()));

    let beacons = async {
        let mut ticks = interval(Duration::from_secs(u64::from(BEACON_INTERVAL_SECONDS)));
        loop {
            ticks.tick().await;
            let beacon = examples_std::csp_demo::Beacon {
                satellite_address: SATELLITE_ADDRESS,
                unix_time_seconds: unix_time_seconds(),
                interval_seconds: BEACON_INTERVAL_SECONDS,
            }
            .encode();
            beacon_435
                .send_to(MAX_NODE_ID, BEACON_PORT, &beacon)
                .await
                .unwrap();
            beacon_868
                .send_to(MAX_NODE_ID, BEACON_PORT, &beacon)
                .await
                .unwrap();
            println!("satellite: beacon queued on 435 and 868 MHz");
        }
    };

    let collector_records = Arc::clone(&records);
    let collect_client_data = async move {
        loop {
            let datagram = client_data.recv_from().await;
            let mut records = collector_records.lock().await;
            records.push(OpaqueRecord {
                source: datagram.source(),
                source_port: datagram.source_port(),
                payload: datagram.payload().to_vec(),
            });
            println!(
                "satellite: stored opaque record from {}:{}, {} bytes; total={}",
                datagram.source(),
                datagram.source_port(),
                datagram.payload().len(),
                records.len()
            );
        }
    };

    let command_records = Arc::clone(&records);
    let command_service = async move {
        let mut connection = command_listener.accept().await.unwrap();
        println!("satellite: ground RDP connection accepted");
        let command = connection.recv().await;
        if command.payload() != DUMP_COMMAND {
            println!("satellite: unknown command {:02X?}", command.payload());
        }

        let response = {
            let records = command_records.lock().await;
            encode_dump(&records)
        };
        let included = response.get(1).copied().unwrap_or(0);
        connection.send(&response).await.unwrap();
        println!("satellite: sent {included} opaque records to ground");

        pending::<()>().await;
    };

    let clock = async {
        let mut now_ms = 0_u32;
        let mut ticks = interval(Duration::from_millis(10));
        loop {
            ticks.tick().await;
            now_ms = now_ms.wrapping_add(10);
            rdp_435.update_time(now_ms);
        }
    };

    println!("satellite: running; UDP mocks represent independent 435/868 MHz radios");
    tokio::select! {
        result = runner_435.run() => panic!("435 Runner stopped: {result:?}"),
        result = runner_868.run() => panic!("868 Runner stopped: {result:?}"),
        () = beacons => unreachable!(),
        () = collect_client_data => unreachable!(),
        () = command_service => unreachable!(),
        () = clock => unreachable!(),
    }
}

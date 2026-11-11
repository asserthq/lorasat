use sat_csp::{RunnerBuilder, StackResources};
use test_utils::host::{RadioBand, UdpCspInterface};

const MTU: usize = 255;

#[tokio::test]
async fn real_csp_datagrams_cross_udp_interface() {
    let interface_a = UdpCspInterface::bind(RadioBand::Mhz435, 611, &[612])
        .await
        .unwrap();
    let interface_b = UdpCspInterface::bind(RadioBand::Mhz435, 612, &[611])
        .await
        .unwrap();
    let stack_a = StackResources::<MTU, 2, 1, 2>::new();
    let stack_b = StackResources::<MTU, 2, 1, 2>::new();

    let mut builder_a = RunnerBuilder::new(611, interface_a, &stack_a).unwrap();
    let mut socket_a = builder_a.bind(10).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(612, interface_b, &stack_b).unwrap();
    let mut socket_b = builder_b.bind(20).unwrap();
    let mut runner_b = builder_b.build();

    let application = async {
        socket_a.send_to(612, 20, b"request").await.unwrap();
        let request = socket_b.recv_from().await;
        assert_eq!(request.source(), 611);
        assert_eq!(request.source_port(), 10);
        assert_eq!(request.payload(), b"request");

        socket_b.send_to(611, 10, b"response").await.unwrap();
        let response = socket_a.recv_from().await;
        assert_eq!(response.source(), 612);
        assert_eq!(response.source_port(), 20);
        assert_eq!(response.payload(), b"response");
    };

    tokio::select! {
        () = application => {}
        result = runner_a.run() => panic!("runner A stopped: {result:?}"),
        result = runner_b.run() => panic!("runner B stopped: {result:?}"),
    }
}

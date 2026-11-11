use std::{fmt::Debug, future::Future, net::SocketAddr, time::Duration};

use sat_csp::{RunnerBuilder, StackResources};
use test_utils::host::UdpCspInterface;

const MTU: usize = 255;
const NODE_A: u16 = 1;
const NODE_B: u16 = 2;
const PORT_A: u8 = 10;
const PORT_B: u8 = 20;

async fn udp_pair() -> (UdpCspInterface, UdpCspInterface) {
    let loopback = SocketAddr::from(([127, 0, 0, 1], 0));
    let mut interface_a = UdpCspInterface::bind_udp(loopback, &[]).await.unwrap();
    let mut interface_b = UdpCspInterface::bind_udp(loopback, &[]).await.unwrap();
    interface_a.set_peers(&[interface_b.local_addr().unwrap()]);
    interface_b.set_peers(&[interface_a.local_addr().unwrap()]);
    (interface_a, interface_b)
}

async fn complete_exchange<Application, RunnerA, RunnerB>(
    application: Application,
    runner_a: RunnerA,
    runner_b: RunnerB,
) where
    Application: Future<Output = ()>,
    RunnerA: Future,
    RunnerA::Output: Debug,
    RunnerB: Future,
    RunnerB::Output: Debug,
{
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            () = application => {}
            result = runner_a => panic!("runner A stopped unexpectedly: {result:?}"),
            result = runner_b => panic!("runner B stopped unexpectedly: {result:?}"),
        }
    })
    .await
    .expect("CSP exchange over UDP timed out");
}

#[tokio::test]
async fn sends_one_datagram_between_nodes_and_ports() {
    let (interface_a, interface_b) = udp_pair().await;
    let resources_a = StackResources::<MTU, 1, 1, 1>::new();
    let resources_b = StackResources::<MTU, 1, 1, 1>::new();

    let mut builder_a = RunnerBuilder::new(NODE_A, interface_a, &resources_a).unwrap();
    let socket_a = builder_a.bind(PORT_A).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(NODE_B, interface_b, &resources_b).unwrap();
    let mut socket_b = builder_b.bind(PORT_B).unwrap();
    let mut runner_b = builder_b.build();

    let exchange = async {
        socket_a
            .send_to(NODE_B, PORT_B, b"one datagram")
            .await
            .unwrap();

        let datagram = socket_b.recv_from().await;
        assert_eq!(datagram.source(), NODE_A);
        assert_eq!(datagram.source_port(), PORT_A);
        assert_eq!(datagram.destination(), NODE_B);
        assert_eq!(datagram.destination_port(), PORT_B);
        assert_eq!(datagram.payload(), b"one datagram");
    };

    complete_exchange(exchange, runner_a.run(), runner_b.run()).await;
}

#[tokio::test]
async fn replies_to_the_source_node_and_port() {
    let (interface_a, interface_b) = udp_pair().await;
    let resources_a = StackResources::<MTU, 1, 1, 1>::new();
    let resources_b = StackResources::<MTU, 1, 1, 1>::new();

    let mut builder_a = RunnerBuilder::new(NODE_A, interface_a, &resources_a).unwrap();
    let mut socket_a = builder_a.bind(PORT_A).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(NODE_B, interface_b, &resources_b).unwrap();
    let mut socket_b = builder_b.bind(PORT_B).unwrap();
    let mut runner_b = builder_b.build();

    let exchange = async {
        socket_a.send_to(NODE_B, PORT_B, b"request").await.unwrap();

        let request = socket_b.recv_from().await;
        assert_eq!(request.payload(), b"request");
        socket_b
            .send_to(request.source(), request.source_port(), b"response")
            .await
            .unwrap();

        let response = socket_a.recv_from().await;
        assert_eq!(response.source(), NODE_B);
        assert_eq!(response.source_port(), PORT_B);
        assert_eq!(response.destination(), NODE_A);
        assert_eq!(response.destination_port(), PORT_A);
        assert_eq!(response.payload(), b"response");
    };

    complete_exchange(exchange, runner_a.run(), runner_b.run()).await;
}

#[tokio::test]
async fn routes_datagrams_to_their_destination_ports() {
    const PORT_B_SECONDARY: u8 = 21;

    let (interface_a, interface_b) = udp_pair().await;
    let resources_a = StackResources::<MTU, 2, 2, 2>::new();
    let resources_b = StackResources::<MTU, 2, 2, 2>::new();

    let mut builder_a = RunnerBuilder::new(NODE_A, interface_a, &resources_a).unwrap();
    let socket_a = builder_a.bind(PORT_A).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(NODE_B, interface_b, &resources_b).unwrap();
    let mut primary = builder_b.bind(PORT_B).unwrap();
    let mut secondary = builder_b.bind(PORT_B_SECONDARY).unwrap();
    let mut runner_b = builder_b.build();

    let exchange = async {
        socket_a.send_to(NODE_B, PORT_B, b"primary").await.unwrap();
        socket_a
            .send_to(NODE_B, PORT_B_SECONDARY, b"secondary")
            .await
            .unwrap();

        let primary_datagram = primary.recv_from().await;
        let secondary_datagram = secondary.recv_from().await;

        assert_eq!(primary_datagram.destination_port(), PORT_B);
        assert_eq!(primary_datagram.payload(), b"primary");
        assert_eq!(secondary_datagram.destination_port(), PORT_B_SECONDARY);
        assert_eq!(secondary_datagram.payload(), b"secondary");
        assert!(primary.try_recv_from().is_none());
        assert!(secondary.try_recv_from().is_none());
    };

    complete_exchange(exchange, runner_a.run(), runner_b.run()).await;
}

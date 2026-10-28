mod support;

use futures::{
    FutureExt,
    future::{Either, join, select},
    pin_mut,
};
use sat_csp::{RunnerBuilder, StackResources};
use support::fake_link_pair;

#[test]
fn runner_routes_ports_and_survives_discarded_frames() {
    let ((link_a, mut control_a), (link_b, _control_b)) = fake_link_pair(16);
    let resources_a = StackResources::<255, 4, 1, 2>::new();
    let resources_b = StackResources::<255, 4, 2, 2>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &resources_a).unwrap();
    let socket_a = builder_a.bind(10).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &resources_b).unwrap();
    let mut socket_b20 = builder_b.bind(20).unwrap();
    let mut socket_b21 = builder_b.bind(21).unwrap();
    let mut runner_b = builder_b.build();

    futures::executor::block_on(async {
        let application = async {
            socket_a.send_to(2, 20, b"port 20").await.unwrap();
            socket_a.send_to(2, 21, b"port 21").await.unwrap();
            assert_eq!(socket_b20.recv_from().await.payload(), b"port 20");
            assert_eq!(socket_b21.recv_from().await.payload(), b"port 21");

            control_a.inject(&[0; 5]).await.unwrap();
            socket_a.send_to(2, 21, b"after malformed").await.unwrap();
            assert_eq!(socket_b21.recv_from().await.payload(), b"after malformed");

            socket_a.send_to(3, 20, b"wrong destination").await.unwrap();
            socket_a.send_to(2, 21, b"after wrong").await.unwrap();
            assert_eq!(socket_b21.recv_from().await.payload(), b"after wrong");

            socket_a.send_to(2, 22, b"closed port").await.unwrap();
            socket_a.send_to(2, 21, b"after closed").await.unwrap();
            assert_eq!(socket_b21.recv_from().await.payload(), b"after closed");

            assert!(socket_b20.try_recv_from().is_none());
            assert!(socket_b21.try_recv_from().is_none());
        }
        .fuse();

        let runners = join(runner_a.run(), runner_b.run()).fuse();
        pin_mut!(application, runners);
        match select(application, runners).await {
            Either::Left(((), _remaining_runners)) => {}
            Either::Right((result, _)) => panic!("runner stopped unexpectedly: {result:?}"),
        }
    });

    let stats = runner_b.stats();
    assert_eq!(stats.decode_errors, 1);
    assert_eq!(stats.wrong_destination, 1);
    assert_eq!(stats.no_socket, 1);
}

#[test]
fn full_socket_queue_drops_newest_without_blocking_runner() {
    let ((link_a, _control_a), (link_b, _control_b)) = fake_link_pair(8);
    let resources_a = StackResources::<255, 4, 1, 1>::new();
    let resources_b = StackResources::<255, 4, 2, 1>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &resources_a).unwrap();
    let socket_a = builder_a.bind(10).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &resources_b).unwrap();
    let mut data_socket = builder_b.bind(20).unwrap();
    let mut barrier_socket = builder_b.bind(21).unwrap();
    let mut runner_b = builder_b.build();

    futures::executor::block_on(async {
        let application = async {
            socket_a.send_to(2, 20, b"first").await.unwrap();
            socket_a.send_to(2, 20, b"second").await.unwrap();
            socket_a.send_to(2, 21, b"barrier").await.unwrap();

            assert_eq!(barrier_socket.recv_from().await.payload(), b"barrier");
            assert_eq!(data_socket.recv_from().await.payload(), b"first");
            assert!(data_socket.try_recv_from().is_none());
        }
        .fuse();

        let runners = join(runner_a.run(), runner_b.run()).fuse();
        pin_mut!(application, runners);
        match select(application, runners).await {
            Either::Left(((), _remaining_runners)) => {}
            Either::Right((result, _)) => panic!("runner stopped unexpectedly: {result:?}"),
        }
    });

    assert_eq!(runner_b.stats().rx_queue_full, 1);
}

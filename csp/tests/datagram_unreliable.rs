mod support;

use futures::{
    FutureExt,
    future::{Either, join, select},
    pin_mut,
};
use sat_csp::{RunnerBuilder, SendError, StackResources};
use support::fake_link_pair;

#[test]
fn loss_has_no_retry_and_duplicate_is_delivered_twice() {
    let ((link_a, mut control_a), (link_b, mut control_b)) = fake_link_pair(8);
    let resources_a = StackResources::<255, 2, 1, 4>::new();
    let resources_b = StackResources::<255, 2, 1, 4>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &resources_a).unwrap();
    let mut socket_a = builder_a.bind(10).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &resources_b).unwrap();
    let mut socket_b = builder_b.bind(20).unwrap();
    let mut runner_b = builder_b.build();

    futures::executor::block_on(async {
        let application = async {
            control_a.drop_next();
            socket_a.send_to(2, 20, b"lost request").await.unwrap();
            control_a.wait_for_transmission().await;
            assert_eq!(control_a.transmission_count(), 1);
            assert!(socket_b.try_recv_from().is_none());

            socket_a.send_to(2, 20, b"delivered request").await.unwrap();
            assert_eq!(socket_b.recv_from().await.payload(), b"delivered request");

            control_b.drop_next();
            socket_b.send_to(1, 10, b"lost reply").await.unwrap();
            control_b.wait_for_transmission().await;
            assert_eq!(control_b.transmission_count(), 1);
            assert!(socket_a.try_recv_from().is_none());

            control_a.duplicate_next();
            socket_a.send_to(2, 20, b"duplicate").await.unwrap();
            assert_eq!(socket_b.recv_from().await.payload(), b"duplicate");
            assert_eq!(socket_b.recv_from().await.payload(), b"duplicate");
            assert_eq!(control_a.transmission_count(), 3);
        }
        .fuse();

        let runners = join(runner_a.run(), runner_b.run()).fuse();
        pin_mut!(application, runners);
        match select(application, runners).await {
            Either::Left(((), _remaining_runners)) => {}
            Either::Right((result, _)) => panic!("runner stopped unexpectedly: {result:?}"),
        }
    });
}

#[test]
fn concurrent_datagrams_flow_in_both_directions() {
    let ((link_a, control_a), (link_b, control_b)) = fake_link_pair(8);
    let resources_a = StackResources::<255, 2, 1, 2>::new();
    let resources_b = StackResources::<255, 2, 1, 2>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &resources_a).unwrap();
    let mut socket_a = builder_a.bind(10).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &resources_b).unwrap();
    let mut socket_b = builder_b.bind(20).unwrap();
    let mut runner_b = builder_b.build();

    futures::executor::block_on(async {
        let application = async {
            let side_a = async {
                socket_a.send_to(2, 20, b"A to B").await.unwrap();
                let received = socket_a.recv_from().await;
                assert_eq!(received.source(), 2);
                assert_eq!(received.payload(), b"B to A");
            };
            let side_b = async {
                socket_b.send_to(1, 10, b"B to A").await.unwrap();
                let received = socket_b.recv_from().await;
                assert_eq!(received.source(), 1);
                assert_eq!(received.payload(), b"A to B");
            };
            join(side_a, side_b).await;
        }
        .fuse();

        let runners = join(runner_a.run(), runner_b.run()).fuse();
        pin_mut!(application, runners);
        match select(application, runners).await {
            Either::Left(((), _remaining_runners)) => {}
            Either::Right((result, _)) => panic!("runner stopped unexpectedly: {result:?}"),
        }
    });

    assert_eq!(control_a.transmission_count(), 1);
    assert_eq!(control_b.transmission_count(), 1);
}

#[test]
fn full_tx_queue_recovers_and_rx_still_runs() {
    let ((link_a, _control_a), (link_b, _control_b)) = fake_link_pair(8);
    let resources_a = StackResources::<255, 1, 1, 2>::new();
    let resources_b = StackResources::<255, 1, 1, 2>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &resources_a).unwrap();
    let mut socket_a = builder_a.bind(10).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &resources_b).unwrap();
    let mut socket_b = builder_b.bind(20).unwrap();
    let mut runner_b = builder_b.build();

    socket_a.try_send_to(2, 20, b"queued").unwrap();
    assert_eq!(socket_a.try_send_to(2, 20, b"busy"), Err(SendError::Busy));

    futures::executor::block_on(async {
        let application = async {
            let wait_for_tx_space = socket_a.send_to(2, 20, b"waited");
            let send_incoming = socket_b.send_to(1, 10, b"incoming");
            let (waited_result, incoming_result) = join(wait_for_tx_space, send_incoming).await;
            waited_result.unwrap();
            incoming_result.unwrap();

            assert_eq!(socket_b.recv_from().await.payload(), b"queued");
            assert_eq!(socket_b.recv_from().await.payload(), b"waited");
            assert_eq!(socket_a.recv_from().await.payload(), b"incoming");
        }
        .fuse();

        let runners = join(runner_a.run(), runner_b.run()).fuse();
        pin_mut!(application, runners);
        match select(application, runners).await {
            Either::Left(((), _remaining_runners)) => {}
            Either::Right((result, _)) => panic!("runner stopped unexpectedly: {result:?}"),
        }
    });
}

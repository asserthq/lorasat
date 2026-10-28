mod support;

use futures::{
    FutureExt,
    future::{Either, join, select},
    pin_mut,
};
use sat_csp::{RunnerBuilder, StackResources};
use support::fake_link_pair;

#[test]
fn datagram_request_reply() {
    let ((link_a, control_a), (link_b, control_b)) = fake_link_pair(4);
    let resources_a = StackResources::<255, 2, 2, 2>::new();
    let resources_b = StackResources::<255, 2, 2, 2>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &resources_a).unwrap();
    let mut socket_a = builder_a.bind(10).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &resources_b).unwrap();
    let mut socket_b = builder_b.bind(20).unwrap();
    let mut runner_b = builder_b.build();

    futures::executor::block_on(async {
        let application = async {
            socket_a.send_to(2, 20, b"hello").await.unwrap();

            let request = socket_b.recv_from().await;
            assert_eq!(request.source(), 1);
            assert_eq!(request.source_port(), 10);
            assert_eq!(request.destination(), 2);
            assert_eq!(request.destination_port(), 20);
            assert_eq!(request.payload(), b"hello");

            socket_b
                .send_to(request.source(), request.source_port(), b"world")
                .await
                .unwrap();

            let reply = socket_a.recv_from().await;
            assert_eq!(reply.source(), 2);
            assert_eq!(reply.source_port(), 20);
            assert_eq!(reply.destination(), 1);
            assert_eq!(reply.destination_port(), 10);
            assert_eq!(reply.payload(), b"world");
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
    assert_eq!(
        control_a.transmitted_frames(),
        [vec![
            0x80, 0x02, 0x00, 0x05, 0x42, 0x80, b'h', b'e', b'l', b'l', b'o',
        ]]
    );
    assert_eq!(control_b.transmission_count(), 1);
    assert_eq!(
        control_b.transmitted_frames(),
        [vec![
            0x80, 0x01, 0x00, 0x08, 0xA5, 0x00, b'w', b'o', b'r', b'l', b'd',
        ]]
    );
    assert_eq!(runner_a.stats().transmitted_frames, 1);
    assert_eq!(runner_a.stats().received_frames, 1);
    assert_eq!(runner_b.stats().transmitted_frames, 1);
    assert_eq!(runner_b.stats().received_frames, 1);
}

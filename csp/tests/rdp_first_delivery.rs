mod support;

use futures::{
    FutureExt,
    future::{Either, join, select},
    pin_mut,
};
use sat_csp::{Flags, Packet, Priority, RdpConfig, RdpResources, RunnerBuilder, StackResources};
use support::fake_link_pair;

const MTU: usize = 255;

#[test]
fn first_rdp_connection_confirms_one_delivery() {
    let ((link_a, control_a), (link_b, mut control_b)) = fake_link_pair(8);
    let stack_a = StackResources::<MTU, 2, 1, 1>::new();
    let stack_b = StackResources::<MTU, 2, 1, 1>::new();
    let rdp_a = RdpResources::<MTU>::new();
    let rdp_b = RdpResources::<MTU>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &stack_a).unwrap();
    let client = builder_a
        .rdp_client(&rdp_a, RdpConfig::new(0x1000))
        .unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &stack_b).unwrap();
    let listener = builder_b
        .listen_rdp(20, &rdp_b, RdpConfig::new(0x2000))
        .unwrap();
    let mut runner_b = builder_b.build();

    futures::executor::block_on(async {
        let application = async {
            let (client_connection, server_connection) =
                join(client.connect(2, 20), listener.accept()).await;
            let mut client_connection = client_connection.unwrap();
            let mut server_connection = server_connection.unwrap();

            assert_eq!(client_connection.peer_address(), 2);
            assert_eq!(client_connection.local_port(), 63);
            assert_eq!(client_connection.peer_port(), 20);
            assert_eq!(server_connection.peer_address(), 1);
            assert_eq!(server_connection.local_port(), 20);
            assert_eq!(server_connection.peer_port(), 63);

            control_b.hold_next();
            let send = client_connection.send(b"command").fuse();
            let receive = server_connection.recv().fuse();
            pin_mut!(send, receive);

            let (message, remaining_send) = match select(send, receive).await {
                Either::Left((result, _)) => {
                    panic!("send completed before the delivery ACK was released: {result:?}")
                }
                Either::Right((message, remaining_send)) => (message, remaining_send),
            };
            assert_eq!(message.source(), 1);
            assert_eq!(message.source_port(), 63);
            assert_eq!(message.payload(), b"command");

            control_b.release_held().await.unwrap();
            remaining_send.await.unwrap();
        }
        .fuse();

        let runners = join(runner_a.run(), runner_b.run()).fuse();
        pin_mut!(application, runners);
        match select(application, runners).await {
            Either::Left(((), _remaining_runners)) => {}
            Either::Right((result, _)) => panic!("runner stopped unexpectedly: {result:?}"),
        }
    });

    let frames_a = control_a.transmitted_frames();
    let frames_b = control_b.transmitted_frames();
    assert_eq!(frames_a.len(), 3);
    assert_eq!(frames_b.len(), 2);

    assert_frame(
        &frames_a[0],
        Priority::High,
        1,
        2,
        63,
        20,
        &[
            0x00, 0x00, 0x00, 0x01, // window size
            0x00, 0x00, 0x27, 0x10, // connection timeout
            0x00, 0x00, 0x03, 0xE8, // packet timeout
            0x00, 0x00, 0x00, 0x00, // delayed ACK disabled
            0x00, 0x00, 0x00, 0xFA, // ACK timeout
            0x00, 0x00, 0x00, 0x02, // ACK delay count
            0x08, 0x10, 0x00, 0x00, 0x00, // SYN trailer
        ],
    );
    assert_frame(
        &frames_b[0],
        Priority::High,
        2,
        1,
        20,
        63,
        &[0x0C, 0x20, 0x00, 0x10, 0x00],
    );
    assert_frame(
        &frames_a[1],
        Priority::High,
        1,
        2,
        63,
        20,
        &[0x14, 0x10, 0x01, 0x20, 0x00],
    );

    let mut expected_data = b"command".to_vec();
    expected_data.extend_from_slice(&[0x04, 0x10, 0x01, 0x20, 0x00]);
    assert_frame(&frames_a[2], Priority::Normal, 1, 2, 63, 20, &expected_data);
    assert_frame(
        &frames_b[1],
        Priority::High,
        2,
        1,
        20,
        63,
        &[0x14, 0x20, 0x01, 0x10, 0x01],
    );

    assert_eq!(runner_a.stats().transmitted_frames, 3);
    assert_eq!(runner_a.stats().received_frames, 2);
    assert_eq!(runner_b.stats().transmitted_frames, 2);
    assert_eq!(runner_b.stats().received_frames, 3);
}

#[test]
fn lost_ack_retries_data_without_duplicate_delivery() {
    let ((link_a, control_a), (link_b, mut control_b)) = fake_link_pair(8);
    let stack_a = StackResources::<MTU, 2, 1, 1>::new();
    let stack_b = StackResources::<MTU, 2, 1, 1>::new();
    let rdp_a = RdpResources::<MTU>::new();
    let rdp_b = RdpResources::<MTU>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &stack_a).unwrap();
    let client = builder_a
        .rdp_client(&rdp_a, RdpConfig::new(0x1000))
        .unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &stack_b).unwrap();
    let listener = builder_b
        .listen_rdp(20, &rdp_b, RdpConfig::new(0x2000))
        .unwrap();
    let mut runner_b = builder_b.build();

    futures::executor::block_on(async {
        let application = async {
            let (client_connection, server_connection) =
                join(client.connect(2, 20), listener.accept()).await;
            let mut client_connection = client_connection.unwrap();
            let mut server_connection = server_connection.unwrap();

            control_b.wait_for_transmission().await;
            control_b.drop_next();
            let receive_and_advance = async {
                let message = server_connection.recv().await;
                control_b.wait_for_transmission().await;
                rdp_a.update_time(1_001);
                message
            };
            let (send_result, message) =
                join(client_connection.send(b"command"), receive_and_advance).await;
            send_result.unwrap();
            assert_eq!(message.payload(), b"command");
            assert!(server_connection.try_recv().is_none());
        }
        .fuse();

        let runners = join(runner_a.run(), runner_b.run()).fuse();
        pin_mut!(application, runners);
        match select(application, runners).await {
            Either::Left(((), _remaining_runners)) => {}
            Either::Right((result, _)) => panic!("runner stopped unexpectedly: {result:?}"),
        }
    });

    assert_eq!(control_a.transmission_count(), 4);
    assert_eq!(control_b.transmission_count(), 3);
}

#[test]
fn one_message_each_way_uses_same_connection() {
    let ((link_a, _control_a), (link_b, _control_b)) = fake_link_pair(8);
    let stack_a = StackResources::<MTU, 2, 1, 1>::new();
    let stack_b = StackResources::<MTU, 2, 1, 1>::new();
    let rdp_a = RdpResources::<MTU>::new();
    let rdp_b = RdpResources::<MTU>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &stack_a).unwrap();
    let client = builder_a
        .rdp_client(&rdp_a, RdpConfig::new(0x1000))
        .unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &stack_b).unwrap();
    let listener = builder_b
        .listen_rdp(20, &rdp_b, RdpConfig::new(0x2000))
        .unwrap();
    let mut runner_b = builder_b.build();

    futures::executor::block_on(async {
        let application = async {
            let (client_connection, server_connection) =
                join(client.connect(2, 20), listener.accept()).await;
            let mut client_connection = client_connection.unwrap();
            let mut server_connection = server_connection.unwrap();

            let (client_sent, server_sent) = join(
                client_connection.send(b"command"),
                server_connection.send(b"response"),
            )
            .await;
            client_sent.unwrap();
            server_sent.unwrap();

            assert_eq!(client_connection.recv().await.payload(), b"response");
            assert_eq!(server_connection.recv().await.payload(), b"command");
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
fn data_completes_handshake_when_final_ack_is_lost() {
    let ((link_a, mut control_a), (link_b, control_b)) = fake_link_pair(8);
    let stack_a = StackResources::<MTU, 2, 1, 1>::new();
    let stack_b = StackResources::<MTU, 2, 1, 1>::new();
    let rdp_a = RdpResources::<MTU>::new();
    let rdp_b = RdpResources::<MTU>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &stack_a).unwrap();
    let client = builder_a
        .rdp_client(&rdp_a, RdpConfig::new(0x1000))
        .unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &stack_b).unwrap();
    let listener = builder_b
        .listen_rdp(20, &rdp_b, RdpConfig::new(0x2000))
        .unwrap();
    let mut runner_b = builder_b.build();

    futures::executor::block_on(async {
        let application = async {
            let client_flow = async {
                let mut connection = client.connect(2, 20).await.unwrap();
                connection.send(b"command").await.unwrap();
            };
            let server_flow = async {
                let mut connection = listener.accept().await.unwrap();
                connection.recv().await
            };
            let drop_final_ack = async {
                control_a.wait_for_transmission().await;
                control_a.drop_next();
            };

            let (((), message), ()) = join(join(client_flow, server_flow), drop_final_ack).await;
            assert_eq!(message.payload(), b"command");
        }
        .fuse();

        let runners = join(runner_a.run(), runner_b.run()).fuse();
        pin_mut!(application, runners);
        match select(application, runners).await {
            Either::Left(((), _remaining_runners)) => {}
            Either::Right((result, _)) => panic!("runner stopped unexpectedly: {result:?}"),
        }
    });

    assert_eq!(control_a.transmission_count(), 3);
    assert_eq!(control_b.transmission_count(), 2);
}

#[test]
fn lost_syn_is_retried() {
    let ((link_a, mut control_a), (link_b, control_b)) = fake_link_pair(8);
    let stack_a = StackResources::<MTU, 2, 1, 1>::new();
    let stack_b = StackResources::<MTU, 2, 1, 1>::new();
    let rdp_a = RdpResources::<MTU>::new();
    let rdp_b = RdpResources::<MTU>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &stack_a).unwrap();
    let client = builder_a
        .rdp_client(&rdp_a, RdpConfig::new(0x1000))
        .unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &stack_b).unwrap();
    let listener = builder_b
        .listen_rdp(20, &rdp_b, RdpConfig::new(0x2000))
        .unwrap();
    let mut runner_b = builder_b.build();

    control_a.drop_next();
    futures::executor::block_on(async {
        let application = async {
            let establish = join(client.connect(2, 20), listener.accept());
            let advance_time = async {
                control_a.wait_for_transmission().await;
                rdp_a.update_time(1_001);
            };
            let ((client_connection, server_connection), ()) = join(establish, advance_time).await;
            assert!(client_connection.is_ok());
            assert!(server_connection.is_ok());
        }
        .fuse();

        let runners = join(runner_a.run(), runner_b.run()).fuse();
        pin_mut!(application, runners);
        match select(application, runners).await {
            Either::Left(((), _remaining_runners)) => {}
            Either::Right((result, _)) => panic!("runner stopped unexpectedly: {result:?}"),
        }
    });

    assert_eq!(control_a.transmission_count(), 3);
    assert_eq!(control_b.transmission_count(), 1);
}

#[test]
fn connection_attempt_has_bounded_timeout() {
    let ((link_a, mut control_a), (link_b, _control_b)) = fake_link_pair(8);
    let stack_a = StackResources::<MTU, 2, 1, 1>::new();
    let stack_b = StackResources::<MTU, 2, 1, 1>::new();
    let rdp_a = RdpResources::<MTU>::new();
    let rdp_b = RdpResources::<MTU>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &stack_a).unwrap();
    let client = builder_a
        .rdp_client(&rdp_a, RdpConfig::new(0x1000))
        .unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &stack_b).unwrap();
    let _listener = builder_b
        .listen_rdp(20, &rdp_b, RdpConfig::new(0x2000))
        .unwrap();
    let mut runner_b = builder_b.build();

    control_a.drop_next();
    futures::executor::block_on(async {
        let application = async {
            let connect = client.connect(2, 20);
            let advance_time = async {
                control_a.wait_for_transmission().await;
                rdp_a.update_time(10_001);
            };
            let (result, ()) = join(connect, advance_time).await;
            match result {
                Err(sat_csp::RdpError::TimedOut) => {}
                Err(error) => panic!("unexpected connect error: {error:?}"),
                Ok(_) => panic!("connection unexpectedly succeeded"),
            }
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

#[allow(clippy::too_many_arguments)]
fn assert_frame(
    frame: &[u8],
    priority: Priority,
    source: u16,
    destination: u16,
    source_port: u8,
    destination_port: u8,
    payload: &[u8],
) {
    let packet = Packet::<MTU>::decode(frame).unwrap();
    let header = packet.header();
    assert_eq!(header.priority(), priority);
    assert_eq!(header.source(), source);
    assert_eq!(header.destination(), destination);
    assert_eq!(header.source_port(), source_port);
    assert_eq!(header.destination_port(), destination_port);
    assert_eq!(header.flags(), Flags::RDP);
    assert_eq!(packet.payload(), payload);
}

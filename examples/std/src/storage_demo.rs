use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use sat_core::{
    net::{Address, DatagramMetadata, DatagramSocket, Endpoint},
    storage::QueueStorage,
};

pub const SENDER_ADDRESS: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25_041);
pub const RECEIVER_ADDRESS: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25_042);
pub const SENDER_NODE: u16 = 1;
pub const RECEIVER_NODE: u16 = 2;
pub const SENDER_PORT: u8 = 10;
pub const RECEIVER_PORT: u8 = 11;
pub const RECEIVER_ENDPOINT: Endpoint =
    Endpoint::new(Address::new_unicast(RECEIVER_NODE).unwrap(), RECEIVER_PORT);
pub const ENTRY_SIZE: usize = 256;
pub const QUEUE_DEPTH: usize = 16;
pub const MTU: usize = sat_csp::HEADER_SIZE + ENTRY_SIZE;
pub const RECEIVE_BUFFER_SIZE: usize = MTU - sat_csp::HEADER_SIZE;
pub const SEND_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum ReceiveStoreError<TransportError, StorageError> {
    Transport(TransportError),
    Storage(StorageError),
}

pub async fn receive_and_store<D: DatagramSocket, S: QueueStorage>(
    socket: &mut D,
    storage: &mut S,
    buffer: &mut [u8],
) -> Result<DatagramMetadata, ReceiveStoreError<D::Error, S::Error>> {
    let metadata = socket
        .recv_from(buffer)
        .await
        .map_err(ReceiveStoreError::Transport)?;
    storage
        .append(&buffer[..metadata.length()])
        .await
        .map_err(ReceiveStoreError::Storage)?;
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csp_demo::{CspDatagramError, CspDatagramSocket};
    use sat_csp::{Runner, RunnerBuilder, StackResources};
    use sat_drivers::storage::{RamStorage, StorageError};
    use test_utils::host::UdpCspInterface;
    use tokio::time::timeout;

    type Resources = StackResources<MTU, 2, 1, 2>;
    type DemoSocket<'a> = CspDatagramSocket<'a, MTU, 2, 2>;
    type DemoRunner<'a> = Runner<'a, UdpCspInterface, MTU, 2, 1, 2>;

    async fn bind_pair<'a>(
        sender_resources: &'a Resources,
        receiver_resources: &'a Resources,
    ) -> (
        DemoSocket<'a>,
        DemoSocket<'a>,
        DemoRunner<'a>,
        DemoRunner<'a>,
    ) {
        let local = SocketAddr::from(([127, 0, 0, 1], 0));
        let receiver = UdpCspInterface::bind_udp(local, &[]).await.unwrap();
        let sender = UdpCspInterface::bind_udp(local, &[receiver.local_addr().unwrap()])
            .await
            .unwrap();
        let mut sender = RunnerBuilder::new(SENDER_NODE, sender, sender_resources).unwrap();
        let mut receiver = RunnerBuilder::new(RECEIVER_NODE, receiver, receiver_resources).unwrap();
        let sender_socket = CspDatagramSocket::new(sender.bind(SENDER_PORT).unwrap());
        let receiver_socket = CspDatagramSocket::new(receiver.bind(RECEIVER_PORT).unwrap());
        (
            sender_socket,
            receiver_socket,
            sender.build(),
            receiver.build(),
        )
    }

    async fn run_pair(
        sender: &mut DemoRunner<'_>,
        receiver: &mut DemoRunner<'_>,
        application: impl std::future::Future<Output = ()>,
    ) {
        timeout(Duration::from_secs(3), async {
            tokio::select! {
                () = application => {}
                result = sender.run() => panic!("sender runner stopped: {result:?}"),
                result = receiver.run() => panic!("receiver runner stopped: {result:?}"),
            }
        })
        .await
        .expect("CSP over UDP test timed out");
    }

    #[tokio::test]
    async fn csp_payloads_are_stored_as_complete_fifo_entries() {
        let sender_resources = Resources::new();
        let receiver_resources = Resources::new();
        let (mut sender, mut receiver, mut sender_runner, mut receiver_runner) =
            bind_pair(&sender_resources, &receiver_resources).await;
        run_pair(&mut sender_runner, &mut receiver_runner, async {
            let mut storage = RamStorage::<32, 4>::new();
            let mut buffer = [0; RECEIVE_BUFFER_SIZE];
            let packets: [&[u8]; 3] = [b"first\0\xff", b"", b"second packet"];
            for packet in packets {
                sender.send_to(RECEIVER_ENDPOINT, packet).await.unwrap();
                let metadata = receive_and_store(&mut receiver, &mut storage, &mut buffer)
                    .await
                    .unwrap();
                assert_eq!(metadata.length(), packet.len());
                assert_eq!(
                    metadata.source(),
                    Endpoint::new(Address::new_unicast(1).unwrap(), SENDER_PORT)
                );
            }
            assert_eq!(storage.len(), packets.len());
            for packet in packets {
                for _ in 0..2 {
                    let metadata = storage.peek(&mut buffer).await.unwrap().unwrap();
                    assert_eq!(&buffer[..metadata.length()], packet);
                }
                assert!(storage.pop().await.unwrap());
            }
            assert!(storage.peek(&mut buffer).await.unwrap().is_none());
        })
        .await;
    }

    #[tokio::test]
    async fn full_queue_rejects_new_datagram_without_overwriting_old_entry() {
        let sender_resources = Resources::new();
        let receiver_resources = Resources::new();
        let (mut sender, mut receiver, mut sender_runner, mut receiver_runner) =
            bind_pair(&sender_resources, &receiver_resources).await;
        run_pair(&mut sender_runner, &mut receiver_runner, async {
            let mut storage = RamStorage::<32, 1>::new();
            let mut buffer = [0; RECEIVE_BUFFER_SIZE];
            sender
                .send_to(RECEIVER_ENDPOINT, b"retained")
                .await
                .unwrap();
            receive_and_store(&mut receiver, &mut storage, &mut buffer)
                .await
                .unwrap();
            for _ in 0..3 {
                sender
                    .send_to(RECEIVER_ENDPOINT, b"rejected")
                    .await
                    .unwrap();
                assert!(matches!(
                    receive_and_store(&mut receiver, &mut storage, &mut buffer).await,
                    Err(ReceiveStoreError::Storage(StorageError::Full))
                ));
                assert_eq!(storage.len(), 1);
                let metadata = storage.peek(&mut buffer).await.unwrap().unwrap();
                assert_eq!(&buffer[..metadata.length()], b"retained");
            }
            assert!(storage.pop().await.unwrap());
            sender.send_to(RECEIVER_ENDPOINT, b"later").await.unwrap();
            receive_and_store(&mut receiver, &mut storage, &mut buffer)
                .await
                .unwrap();
            let metadata = storage.peek(&mut buffer).await.unwrap().unwrap();
            assert_eq!(&buffer[..metadata.length()], b"later");
        })
        .await;
    }

    #[tokio::test]
    async fn oversized_record_is_rejected_and_next_datagram_can_be_stored() {
        let sender_resources = Resources::new();
        let receiver_resources = Resources::new();
        let (mut sender, mut receiver, mut sender_runner, mut receiver_runner) =
            bind_pair(&sender_resources, &receiver_resources).await;
        run_pair(&mut sender_runner, &mut receiver_runner, async {
            let mut storage = RamStorage::<32, 2>::new();
            let mut buffer = [0; RECEIVE_BUFFER_SIZE];
            sender
                .send_to(RECEIVER_ENDPOINT, &[0xff; 33])
                .await
                .unwrap();
            assert!(matches!(
                receive_and_store(&mut receiver, &mut storage, &mut buffer).await,
                Err(ReceiveStoreError::Storage(StorageError::EntryTooLong {
                    length: 33,
                    maximum: 32,
                }))
            ));
            assert!(storage.is_empty());
            sender.send_to(RECEIVER_ENDPOINT, b"valid").await.unwrap();
            receive_and_store(&mut receiver, &mut storage, &mut buffer)
                .await
                .unwrap();
            let metadata = storage.peek(&mut buffer).await.unwrap().unwrap();
            assert_eq!(&buffer[..metadata.length()], b"valid");
        })
        .await;
    }

    #[tokio::test]
    async fn small_application_buffer_retains_datagram_and_maximum_payload_fits() {
        let sender_resources = Resources::new();
        let receiver_resources = Resources::new();
        let (mut sender, mut receiver, mut sender_runner, mut receiver_runner) =
            bind_pair(&sender_resources, &receiver_resources).await;
        run_pair(&mut sender_runner, &mut receiver_runner, async {
            let mut storage = RamStorage::<ENTRY_SIZE, QUEUE_DEPTH>::new();
            let payload = [0xff; ENTRY_SIZE];
            sender.send_to(RECEIVER_ENDPOINT, &payload).await.unwrap();
            let mut small = [0xa5; 4];
            assert!(matches!(
                receive_and_store(&mut receiver, &mut storage, &mut small).await,
                Err(ReceiveStoreError::Transport(
                    CspDatagramError::BufferTooSmall {
                        required: ENTRY_SIZE,
                        available: 4,
                    }
                ))
            ));
            assert_eq!(small, [0xa5; 4]);
            assert!(storage.is_empty());
            let mut buffer = [0; RECEIVE_BUFFER_SIZE];
            let metadata = receive_and_store(&mut receiver, &mut storage, &mut buffer)
                .await
                .unwrap();
            assert_eq!(metadata.length(), ENTRY_SIZE);
            assert_eq!(buffer, payload);
            let metadata = storage.peek(&mut buffer).await.unwrap().unwrap();
            assert_eq!(metadata.length(), ENTRY_SIZE);
            assert_eq!(buffer, payload);
            assert!(matches!(
                sender
                    .send_to(RECEIVER_ENDPOINT, &[0; ENTRY_SIZE + 1])
                    .await,
                Err(CspDatagramError::Send(sat_csp::SendError::Packet(
                    sat_csp::Error::PacketTooLarge { .. }
                )))
            ));
        })
        .await;
    }

    #[tokio::test]
    async fn csp_filters_other_nodes_and_ports_before_storage() {
        let sender_resources = Resources::new();
        let receiver_resources = Resources::new();
        let (mut sender, mut receiver, mut sender_runner, mut receiver_runner) =
            bind_pair(&sender_resources, &receiver_resources).await;
        run_pair(&mut sender_runner, &mut receiver_runner, async {
            sender
                .send_to(
                    Endpoint::new(Address::new_unicast(3).unwrap(), RECEIVER_PORT),
                    b"other node",
                )
                .await
                .unwrap();
            sender
                .send_to(
                    Endpoint::new(Address::new_unicast(2).unwrap(), RECEIVER_PORT + 1),
                    b"other port",
                )
                .await
                .unwrap();
            sender.send_to(RECEIVER_ENDPOINT, b"ours").await.unwrap();
            let mut storage = RamStorage::<ENTRY_SIZE, QUEUE_DEPTH>::new();
            let mut buffer = [0; RECEIVE_BUFFER_SIZE];
            let metadata = receive_and_store(&mut receiver, &mut storage, &mut buffer)
                .await
                .unwrap();
            assert_eq!(&buffer[..metadata.length()], b"ours");
            assert_eq!(storage.len(), 1);
        })
        .await;
        assert_eq!(receiver_runner.stats().wrong_destination, 1);
        assert_eq!(receiver_runner.stats().no_socket, 1);
    }
}

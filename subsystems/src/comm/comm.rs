use core::future::pending;

use defmt_or_log::*;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Ticker};
use sat_core::{
    comm::{DatagramMetadata, DatagramSocket, ReliableMessageSession, SessionListener},
    entity::Beacon,
    storage::QueueStorage,
};

use super::{CommConfig, CommState, beacon_codec, client_record_codec, command_codec};

const CLIENT_PAYLOAD_MAX_SIZE: usize = 249;
const CLIENT_RECORD_MAX_SIZE: usize = 300;
const BEACON_MAX_SIZE: usize = beacon_codec::ENCODED_SIZE;
const COMMAND_MESSAGE_MAX_SIZE: usize = 244;

pub struct CommSystem<ClientSocket, GroundSocket, CommandListener, ClientStorage>
where
    ClientSocket: DatagramSocket,
    GroundSocket: DatagramSocket,
    CommandListener: SessionListener,
    ClientStorage: QueueStorage,
{
    client_socket: ClientSocket,
    ground_socket: GroundSocket,
    command_listener: CommandListener,
    storage: ClientStorage,
    config: CommConfig,
    state: CommState,
}

impl<ClientSocket, GroundSocket, CommandListener, ClientStorage>
    CommSystem<ClientSocket, GroundSocket, CommandListener, ClientStorage>
where
    ClientSocket: DatagramSocket,
    GroundSocket: DatagramSocket,
    CommandListener: SessionListener,
    ClientStorage: QueueStorage,
{
    pub fn new(
        client_socket: ClientSocket,
        ground_socket: GroundSocket,
        command_listener: CommandListener,
        storage: ClientStorage,
        config: CommConfig,
    ) -> Self {
        Self {
            client_socket,
            ground_socket,
            command_listener,
            storage,
            config,
            state: CommState::default(),
        }
    }

    pub const fn state(&self) -> &CommState {
        &self.state
    }

    pub async fn run(&mut self) -> ! {
        info!("comm system running");
        let mut beacon_ticker = Ticker::every(Duration::from_secs(u64::from(
            self.config.beacon_interval_sec,
        )));
        let mut client_rx_buffer = [0_u8; CLIENT_PAYLOAD_MAX_SIZE];
        let mut command_listener_enabled = true;

        loop {
            let secondary = select(
                beacon_ticker.next(),
                accept_command(&mut self.command_listener, command_listener_enabled),
            );
            match select(
                self.client_socket.recv_from(&mut client_rx_buffer),
                secondary,
            )
            .await
            {
                Either::First(Ok(metadata)) => {
                    self.handle_client_datagram(metadata, &client_rx_buffer[..metadata.length()])
                        .await;
                }
                Either::First(Err(_)) => {
                    error!("client datagram receive failed");
                    self.state.receive_errors = self.state.receive_errors.saturating_add(1);
                }
                Either::Second(Either::First(_)) => self.send_beacons().await,
                Either::Second(Either::Second(Some(Ok(session)))) => {
                    command_listener_enabled = false;
                    self.handle_ground_session(session).await;
                }
                Either::Second(Either::Second(Some(Err(_)))) => {
                    command_listener_enabled = false;
                    error!("ground command accept failed");
                }
                Either::Second(Either::Second(None)) => core::unreachable!(),
            }
        }
    }

    async fn send_beacons(&mut self) {
        let beacon = Beacon {
            sat_addr: self.config.sat_addr,
            interval_sec: self.config.beacon_interval_sec,
            timestamp: Instant::now().as_millis(),
        };
        let mut buffer = [0_u8; BEACON_MAX_SIZE];
        let length = match beacon_codec::encode(&beacon, &mut buffer) {
            Ok(length) => length,
            Err(_) => {
                error!("beacon encoding failed");
                self.state.beacon_encode_errors = self.state.beacon_encode_errors.saturating_add(1);
                return;
            }
        };

        if self
            .client_socket
            .send_to(self.config.client_beacon_destination, &buffer[..length])
            .await
            .is_ok()
        {
            self.state.transmitted_beacons = self.state.transmitted_beacons.saturating_add(1);
        } else {
            error!("client beacon send failed");
            self.state.beacon_send_errors = self.state.beacon_send_errors.saturating_add(1);
        }

        if self
            .ground_socket
            .send_to(self.config.ground_beacon_destination, &buffer[..length])
            .await
            .is_ok()
        {
            self.state.transmitted_beacons = self.state.transmitted_beacons.saturating_add(1);
        } else {
            error!("ground beacon send failed");
            self.state.beacon_send_errors = self.state.beacon_send_errors.saturating_add(1);
        }
    }

    async fn handle_client_datagram(&mut self, metadata: DatagramMetadata, payload: &[u8]) {
        let mut record_buffer = [0_u8; CLIENT_RECORD_MAX_SIZE];
        let length = match client_record_codec::encode(
            metadata.source(),
            Instant::now().as_millis(),
            payload,
            &mut record_buffer,
        ) {
            Ok(length) => length,
            Err(_) => {
                error!("client record encoding failed");
                self.state.dropped_datagrams = self.state.dropped_datagrams.saturating_add(1);
                return;
            }
        };

        match self.storage.append(&record_buffer[..length]).await {
            Ok(()) => {
                self.state.received_datagrams = self.state.received_datagrams.saturating_add(1);
                self.state.last_client = Some(metadata.source());
                info!("client datagram stored");
            }
            Err(_) => {
                error!("client record storage failed");
                self.state.storage_errors = self.state.storage_errors.saturating_add(1);
            }
        }
    }

    async fn handle_ground_session(&mut self, mut session: CommandListener::Session) {
        let mut command_buffer = [0_u8; COMMAND_MESSAGE_MAX_SIZE];
        let command_length = match session.recv(&mut command_buffer).await {
            Ok(length) => length,
            Err(_) => {
                error!("ground command receive failed");
                return;
            }
        };

        if &command_buffer[..command_length] != command_codec::DUMP_COMMAND {
            warn!("unknown ground command");
            return;
        }

        let mut stored_record = [0_u8; CLIENT_RECORD_MAX_SIZE];
        let mut response = [0_u8; COMMAND_MESSAGE_MAX_SIZE];
        let (response_length, remove_after_delivery) =
            match self.storage.peek(&mut stored_record).await {
                Ok(Some(metadata)) => {
                    let record =
                        match client_record_codec::decode(&stored_record[..metadata.length()]) {
                            Ok(record) => record,
                            Err(_) => {
                                error!("stored client record is invalid");
                                return;
                            }
                        };
                    match command_codec::encode_dump_record(&record, &mut response) {
                        Ok(length) => (length, true),
                        Err(_) => {
                            error!("client record does not fit dump response");
                            return;
                        }
                    }
                }
                Ok(None) => match command_codec::encode_empty_dump(&mut response) {
                    Ok(length) => (length, false),
                    Err(_) => core::unreachable!(),
                },
                Err(_) => {
                    error!("client storage read failed");
                    self.state.storage_errors = self.state.storage_errors.saturating_add(1);
                    return;
                }
            };

        if session.send(&response[..response_length]).await.is_err() {
            error!("client dump delivery failed");
            return;
        }

        if remove_after_delivery {
            match self.storage.pop().await {
                Ok(true) => info!("client record delivered to ground"),
                Ok(false) => error!("delivered client record disappeared from storage"),
                Err(_) => {
                    error!("delivered client record removal failed");
                    self.state.storage_errors = self.state.storage_errors.saturating_add(1);
                }
            }
        }
    }
}

async fn accept_command<Listener>(
    listener: &mut Listener,
    enabled: bool,
) -> Option<Result<Listener::Session, Listener::Error>>
where
    Listener: SessionListener,
{
    if enabled {
        Some(listener.accept().await)
    } else {
        pending().await
    }
}

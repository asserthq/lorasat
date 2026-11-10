use defmt_or_log::info;
use embassy_futures::select::{Either, select};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Sender, pubsub::Subscriber,
};
use embassy_time::{Instant, Ticker};
use sat_core::{
    adcs::{AdcsCommand, AdcsEvent},
    eps::{EpsCommand, EpsEvent},
    net::{DatagramSocket, Endpoint, ReliableSession, SessionListener},
    protocol::{Beacon, GroundCommand, GroundCommandResponse},
    storage::QueueStorage,
};

use crate::cdh::config::CdhConfig;

type CmdSender<C> = Sender<'static, CriticalSectionRawMutex, C, 4>;
type EventSub<E> = Subscriber<'static, CriticalSectionRawMutex, E, 4, 4, 4>;

pub struct SubsystemLink<Cmd: 'static, Event: Clone + 'static> {
    pub cmd_tx: CmdSender<Cmd>,
    pub event_rx: EventSub<Event>,
}

pub struct SubsystemsBundle {
    pub adcs: SubsystemLink<AdcsCommand, AdcsEvent>,
    pub eps: SubsystemLink<EpsCommand, EpsEvent>,
}

pub struct CdhSystem<BeaconS, GroundL, PlatformQ, IotQ>
where
    BeaconS: DatagramSocket,
    GroundL: SessionListener,
    PlatformQ: QueueStorage,
    IotQ: QueueStorage,
{
    config: CdhConfig,
    subsystems: SubsystemsBundle,
    beacon_socket: BeaconS,
    ground_listener: GroundL,
    platform_storage: PlatformQ,
    iot_storage: IotQ,
}

impl<BeaconS, GroundL, PlatformQ, IotQ> CdhSystem<BeaconS, GroundL, PlatformQ, IotQ>
where
    BeaconS: DatagramSocket,
    GroundL: SessionListener,
    PlatformQ: QueueStorage,
    IotQ: QueueStorage,
{
    pub fn new(
        config: CdhConfig,
        subsystems: SubsystemsBundle,
        beacon_socket: BeaconS,
        ground_listener: GroundL,
        platform_storage: PlatformQ,
        iot_storage: IotQ,
    ) -> Self {
        Self {
            config,
            subsystems,
            beacon_socket,
            ground_listener,
            platform_storage,
            iot_storage,
        }
    }

    pub async fn run(&mut self) {
        let mut beacon_ticker = Ticker::every(self.config.beacon_interval);

        loop {
            let branch = select(beacon_ticker.next(), self.ground_listener.accept()).await;
            match branch {
                Either::First(_) => {
                    self.send_beacon().await;
                }
                Either::Second(Ok(session)) => {
                    self.handle_ground_session(session).await;
                }
                Either::Second(Err(_)) => {}
            }
        }
    }

    async fn send_beacon(&mut self) {
        let beacon = Beacon {
            sat_addr: self.config.satellite_address,
            interval_sec: self.config.beacon_interval.as_secs() as u16,
            timestamp: Instant::now().as_millis(),
        };
        let mut buf = [0_u8, 64];
        let payload = beacon.encode(&mut buf).unwrap();
        self.beacon_socket
            .send_to(self.beacon_endpoint(), payload)
            .await
            .unwrap();
    }

    async fn handle_ground_session(&mut self, mut session: GroundL::Session) {
        let mut cmd_buf = [0_u8; 255];
        let mut response_buf = [0_u8; 255];

        loop {
            let length = match session.recv(&mut cmd_buf).await {
                Ok(length) => length,
                Err(_) => {
                    break;
                }
            };

            let response = match GroundCommand::decode(&cmd_buf[..length]) {
                Ok(cmd) => self.handle_ground_command(cmd).await,
                Err(_) => GroundCommandResponse::Failed,
            };

            let payload = match response.encode(&mut response_buf) {
                Ok(payload) => payload,
                Err(_) => break,
            };

            if session.send(payload).await.is_err() {
                break;
            }
        }
    }

    async fn handle_ground_command(&mut self, cmd: GroundCommand) -> GroundCommandResponse {
        info!("handling ground command: {:?}", cmd);
        GroundCommandResponse::Received
    }

    const fn beacon_endpoint(&self) -> Endpoint {
        Endpoint::broadcast(self.config.beacon_port)
    }
}

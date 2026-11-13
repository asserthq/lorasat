use defmt_or_log::info;
use embassy_futures::select::{Either4, select4};
use embassy_time::{Instant, Ticker};
use sat_core::{
    adcs::{AdcsCommand, AdcsEvent},
    eps::{EpsCommand, EpsEvent},
    net::{DatagramSocket, Endpoint, ReliableSession, SessionListener},
    protocol::{Beacon, GroundCommand, GroundCommandResponse},
    storage::QueueStorage,
};

use crate::{
    cdh::config::CdhConfig,
    link::ControllerHandle,
    payload::{PayloadCommand, PayloadEvent},
};

pub struct ControllerHandles<'a> {
    pub adcs: ControllerHandle<'a, AdcsCommand, AdcsEvent>,
    pub eps: ControllerHandle<'a, EpsCommand, EpsEvent>,
    pub service: ControllerHandle<'a, PayloadCommand, PayloadEvent>,
}

pub struct CdhSystem<'a, BeaconS, GroundL, PlatformQ, IotQ>
where
    BeaconS: DatagramSocket,
    GroundL: SessionListener,
    PlatformQ: QueueStorage,
    IotQ: QueueStorage,
{
    config: CdhConfig,
    subsystems: ControllerHandles<'a>,
    beacon_socket: BeaconS,
    ground_listener: GroundL,
    platform_storage: PlatformQ,
    iot_storage: IotQ,
}

impl<'a, BeaconS, GroundL, PlatformQ, IotQ> CdhSystem<'a, BeaconS, GroundL, PlatformQ, IotQ>
where
    BeaconS: DatagramSocket,
    GroundL: SessionListener,
    PlatformQ: QueueStorage,
    IotQ: QueueStorage,
{
    pub fn new(
        config: CdhConfig,
        subsystems: ControllerHandles<'a>,
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
            let branch = select4(
                beacon_ticker.next(),
                self.ground_listener.accept(),
                self.subsystems.adcs.event_rx.receive(),
                self.subsystems.eps.event_rx.receive(),
            )
            .await;

            match branch {
                Either4::First(_) => self.send_beacon().await,
                Either4::Second(Ok(session)) => {
                    self.handle_ground_session(session, &mut beacon_ticker)
                        .await;
                }
                Either4::Second(Err(_)) => {}
                Either4::Third(_) => info!("[SAT::CDH] received ADCS event"),
                Either4::Fourth(_) => info!("[SAT::CDH] received EPS event"),
            }
        }
    }

    async fn send_beacon(&mut self) {
        let beacon = Beacon {
            sat_addr: self.config.satellite_address,
            interval_sec: self.config.beacon_interval.as_secs() as u16,
            timestamp: Instant::now().as_millis(),
        };
        let mut buf = [0_u8; 64];
        let payload = beacon.encode(&mut buf).unwrap();
        self.beacon_socket
            .send_to(self.beacon_endpoint(), payload)
            .await
            .unwrap();
        info!("[SAT::CDH] sent beacon");
    }

    async fn handle_ground_session(
        &mut self,
        mut session: GroundL::Session,
        beacon_ticker: &mut Ticker,
    ) {
        let mut cmd_buf = [0_u8; 255];
        let mut response_buf = [0_u8; 255];

        info!("[SAT::CDH] accepted GS session");

        'session: loop {
            let length = loop {
                let branch = select4(
                    beacon_ticker.next(),
                    session.recv(&mut cmd_buf),
                    self.subsystems.adcs.event_rx.receive(),
                    self.subsystems.eps.event_rx.receive(),
                )
                .await;

                match branch {
                    Either4::First(_) => self.send_beacon().await,
                    Either4::Second(Ok(length)) => break length,
                    Either4::Second(Err(_)) => break 'session,
                    Either4::Third(_) => info!("[SAT::CDH] received ADCS event"),
                    Either4::Fourth(_) => info!("[SAT::CDH] received EPS event"),
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

            info!("[SAT::CDH] sent response on GS command {:?}", response);
        }

        info!("[SAT::CDH] GS session closed");
    }

    async fn handle_ground_command(&mut self, cmd: GroundCommand) -> GroundCommandResponse {
        info!("[SAT::CDH] received GS command {:?}", cmd);
        GroundCommandResponse::Received
    }

    const fn beacon_endpoint(&self) -> Endpoint {
        Endpoint::broadcast(self.config.beacon_port)
    }
}

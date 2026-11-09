use core::fmt;

use embassy_futures::select::{Either, Either4, select, select4};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, TrySendError},
};

use crate::rdp::{RdpBinding, RdpCommand, RdpEngine};
use crate::{
    BindError, CspInterface, Error, Flags, HEADER_SIZE, MAX_NODE_ID, MAX_PORT, Packet,
    RDP_CLIENT_PORT, RDP_SYN_OPTIONS_SIZE, RDP_TRAILER_SIZE, RdpClient, RdpConfig, RdpListener,
    RdpResources, Socket,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RunnerStats {
    pub transmitted_frames: usize,
    pub received_frames: usize,
    pub decode_errors: usize,
    pub wrong_destination: usize,
    pub no_socket: usize,
    pub rx_queue_full: usize,
    pub rdp_errors: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum RunnerError<E> {
    Interface(E),
    Codec(Error),
}

enum RunnerEvent<T, E, C> {
    Outgoing(T),
    Incoming(Result<usize, E>),
    Rdp(C),
    Time(u32),
}

#[derive(Clone, Copy)]
struct RdpSetup<'a, const MTU: usize> {
    resources: &'a RdpResources<MTU>,
    binding: RdpBinding,
}

impl<E: fmt::Display> fmt::Display for RunnerError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interface(error) => write!(formatter, "interface error: {error}"),
            Self::Codec(error) => write!(formatter, "codec error: {error}"),
        }
    }
}

pub struct StackResources<
    const MTU: usize,
    const TX_DEPTH: usize,
    const SOCKETS: usize,
    const RX_DEPTH: usize,
> {
    pub(crate) tx: Channel<CriticalSectionRawMutex, Packet<MTU>, TX_DEPTH>,
    pub(crate) rx: [Channel<CriticalSectionRawMutex, Packet<MTU>, RX_DEPTH>; SOCKETS],
}

impl<const MTU: usize, const TX_DEPTH: usize, const SOCKETS: usize, const RX_DEPTH: usize>
    StackResources<MTU, TX_DEPTH, SOCKETS, RX_DEPTH>
{
    pub const fn new() -> Self {
        Self {
            tx: Channel::new(),
            rx: [const { Channel::new() }; SOCKETS],
        }
    }
}

impl<const MTU: usize, const TX_DEPTH: usize, const SOCKETS: usize, const RX_DEPTH: usize> Default
    for StackResources<MTU, TX_DEPTH, SOCKETS, RX_DEPTH>
{
    fn default() -> Self {
        Self::new()
    }
}

pub struct RunnerBuilder<
    'a,
    I,
    const MTU: usize,
    const TX_DEPTH: usize,
    const SOCKETS: usize,
    const RX_DEPTH: usize,
> {
    local_address: u16,
    interface: I,
    resources: &'a StackResources<MTU, TX_DEPTH, SOCKETS, RX_DEPTH>,
    ports: [Option<u8>; SOCKETS],
    rdp: Option<RdpSetup<'a, MTU>>,
}

impl<'a, I, const MTU: usize, const TX_DEPTH: usize, const SOCKETS: usize, const RX_DEPTH: usize>
    RunnerBuilder<'a, I, MTU, TX_DEPTH, SOCKETS, RX_DEPTH>
{
    pub fn new(
        local_address: u16,
        interface: I,
        resources: &'a StackResources<MTU, TX_DEPTH, SOCKETS, RX_DEPTH>,
    ) -> Result<Self, Error> {
        if local_address > MAX_NODE_ID {
            return Err(Error::InvalidSource(local_address));
        }
        if MTU < HEADER_SIZE {
            return Err(Error::MtuTooSmall {
                minimum: HEADER_SIZE,
                actual: MTU,
            });
        }

        Ok(Self {
            local_address,
            interface,
            resources,
            ports: [None; SOCKETS],
            rdp: None,
        })
    }

    pub fn bind(&mut self, port: u8) -> Result<Socket<'a, MTU, TX_DEPTH, RX_DEPTH>, BindError> {
        if port > MAX_PORT {
            return Err(BindError::InvalidPort(port));
        }
        if self.ports.contains(&Some(port)) || self.rdp_port() == Some(port) {
            return Err(BindError::PortInUse(port));
        }

        let slot = self
            .ports
            .iter()
            .position(Option::is_none)
            .ok_or(BindError::NoSocketSlots)?;
        self.ports[slot] = Some(port);

        Ok(Socket::new(
            self.local_address,
            port,
            self.resources.tx.sender(),
            self.resources.rx[slot].receiver(),
        ))
    }

    pub fn rdp_client(
        &mut self,
        resources: &'a RdpResources<MTU>,
        config: RdpConfig,
    ) -> Result<RdpClient<'a, MTU>, BindError> {
        self.check_rdp_setup(RDP_CLIENT_PORT)?;
        self.rdp = Some(RdpSetup {
            resources,
            binding: RdpBinding::Client(config),
        });
        Ok(RdpClient::new(self.local_address, resources))
    }

    pub fn listen_rdp(
        &mut self,
        port: u8,
        resources: &'a RdpResources<MTU>,
        config: RdpConfig,
    ) -> Result<RdpListener<'a, MTU>, BindError> {
        if port > MAX_PORT {
            return Err(BindError::InvalidPort(port));
        }
        self.check_rdp_setup(port)?;
        self.rdp = Some(RdpSetup {
            resources,
            binding: RdpBinding::Server { port, config },
        });
        Ok(RdpListener::new(resources))
    }

    fn check_rdp_setup(&self, port: u8) -> Result<(), BindError> {
        let minimum = HEADER_SIZE + RDP_SYN_OPTIONS_SIZE + RDP_TRAILER_SIZE;
        if MTU < minimum {
            return Err(BindError::RdpMtuTooSmall {
                minimum,
                actual: MTU,
            });
        }
        if self.rdp.is_some() {
            return Err(BindError::RdpAlreadyConfigured);
        }
        if self.ports.contains(&Some(port)) {
            return Err(BindError::PortInUse(port));
        }
        Ok(())
    }

    fn rdp_port(&self) -> Option<u8> {
        self.rdp.map(|setup| match setup.binding {
            RdpBinding::Client(_) => RDP_CLIENT_PORT,
            RdpBinding::Server { port, .. } => port,
        })
    }

    pub fn build(self) -> Runner<'a, I, MTU, TX_DEPTH, SOCKETS, RX_DEPTH> {
        Runner {
            local_address: self.local_address,
            interface: self.interface,
            resources: self.resources,
            ports: self.ports,
            stats: RunnerStats::default(),
            rdp: self
                .rdp
                .map(|setup| RdpEngine::new(self.local_address, setup.resources, setup.binding)),
        }
    }
}

pub struct Runner<
    'a,
    I,
    const MTU: usize,
    const TX_DEPTH: usize,
    const SOCKETS: usize,
    const RX_DEPTH: usize,
> {
    pub(crate) local_address: u16,
    pub(crate) interface: I,
    pub(crate) resources: &'a StackResources<MTU, TX_DEPTH, SOCKETS, RX_DEPTH>,
    pub(crate) ports: [Option<u8>; SOCKETS],
    stats: RunnerStats,
    rdp: Option<RdpEngine<'a, MTU>>,
}

impl<'a, I, const MTU: usize, const TX_DEPTH: usize, const SOCKETS: usize, const RX_DEPTH: usize>
    Runner<'a, I, MTU, TX_DEPTH, SOCKETS, RX_DEPTH>
where
    I: CspInterface,
{
    pub const fn local_address(&self) -> u16 {
        self.local_address
    }

    pub const fn stats(&self) -> RunnerStats {
        self.stats
    }

    /// Gives the supervisor access to interface-specific recovery or diagnostics.
    ///
    /// Call this only while [`Self::run`] is not being polled.
    pub fn interface_mut(&mut self) -> &mut I {
        &mut self.interface
    }

    pub async fn run(&mut self) -> Result<(), RunnerError<I::Error>> {
        let mut frame = [0; MTU];
        let mut prefer_incoming = false;
        let mut rdp_priority = 3_u8;

        loop {
            let event: RunnerEvent<Packet<MTU>, I::Error, RdpCommand<MTU>> =
                if let Some(rdp) = self.rdp.as_ref() {
                    let event = match rdp_priority {
                        0 => match select4(
                            self.interface.receive(&mut frame),
                            self.resources.tx.receive(),
                            rdp.next_command(),
                            rdp.next_time_update(),
                        )
                        .await
                        {
                            Either4::First(result) => RunnerEvent::Incoming(result),
                            Either4::Second(packet) => RunnerEvent::Outgoing(packet),
                            Either4::Third(command) => RunnerEvent::Rdp(command),
                            Either4::Fourth(now_ms) => RunnerEvent::Time(now_ms),
                        },
                        1 => match select4(
                            self.resources.tx.receive(),
                            rdp.next_command(),
                            rdp.next_time_update(),
                            self.interface.receive(&mut frame),
                        )
                        .await
                        {
                            Either4::First(packet) => RunnerEvent::Outgoing(packet),
                            Either4::Second(command) => RunnerEvent::Rdp(command),
                            Either4::Third(now_ms) => RunnerEvent::Time(now_ms),
                            Either4::Fourth(result) => RunnerEvent::Incoming(result),
                        },
                        2 => match select4(
                            rdp.next_command(),
                            rdp.next_time_update(),
                            self.interface.receive(&mut frame),
                            self.resources.tx.receive(),
                        )
                        .await
                        {
                            Either4::First(command) => RunnerEvent::Rdp(command),
                            Either4::Second(now_ms) => RunnerEvent::Time(now_ms),
                            Either4::Third(result) => RunnerEvent::Incoming(result),
                            Either4::Fourth(packet) => RunnerEvent::Outgoing(packet),
                        },
                        _ => match select4(
                            rdp.next_time_update(),
                            self.interface.receive(&mut frame),
                            self.resources.tx.receive(),
                            rdp.next_command(),
                        )
                        .await
                        {
                            Either4::First(now_ms) => RunnerEvent::Time(now_ms),
                            Either4::Second(result) => RunnerEvent::Incoming(result),
                            Either4::Third(packet) => RunnerEvent::Outgoing(packet),
                            Either4::Fourth(command) => RunnerEvent::Rdp(command),
                        },
                    };
                    rdp_priority = (rdp_priority + 1) % 4;
                    event
                } else if prefer_incoming {
                    match select(
                        self.interface.receive(&mut frame),
                        self.resources.tx.receive(),
                    )
                    .await
                    {
                        Either::First(result) => RunnerEvent::Incoming(result),
                        Either::Second(packet) => RunnerEvent::Outgoing(packet),
                    }
                } else {
                    match select(
                        self.resources.tx.receive(),
                        self.interface.receive(&mut frame),
                    )
                    .await
                    {
                        Either::First(packet) => RunnerEvent::Outgoing(packet),
                        Either::Second(result) => RunnerEvent::Incoming(result),
                    }
                };

            match event {
                RunnerEvent::Outgoing(packet) => {
                    self.transmit(packet, &mut frame).await?;
                    prefer_incoming = true;
                }
                RunnerEvent::Incoming(result) => {
                    let length = result.map_err(RunnerError::Interface)?;
                    self.stats.received_frames = self.stats.received_frames.saturating_add(1);
                    prefer_incoming = false;
                    if length > MTU {
                        self.stats.decode_errors = self.stats.decode_errors.saturating_add(1);
                        continue;
                    }
                    if let Some(response) = self.deliver(&frame[..length]) {
                        self.transmit(response, &mut frame).await?;
                        prefer_incoming = true;
                    }
                }
                RunnerEvent::Rdp(command) => {
                    let packet = self
                        .rdp
                        .as_mut()
                        .expect("RDP command requires configured RDP engine")
                        .handle_command(command)
                        .map_err(RunnerError::Codec)?;
                    if let Some(packet) = packet {
                        self.transmit(packet, &mut frame).await?;
                        prefer_incoming = true;
                    }
                }
                RunnerEvent::Time(now_ms) => {
                    let packet = self
                        .rdp
                        .as_mut()
                        .expect("RDP time update requires configured RDP engine")
                        .handle_time_update(now_ms);
                    if let Some(packet) = packet {
                        self.transmit(packet, &mut frame).await?;
                        prefer_incoming = true;
                    }
                }
            }
        }
    }

    async fn transmit(
        &mut self,
        packet: Packet<MTU>,
        frame: &mut [u8; MTU],
    ) -> Result<(), RunnerError<I::Error>> {
        let length = packet.encode(frame).map_err(RunnerError::Codec)?;
        self.interface
            .send(&frame[..length])
            .await
            .map_err(RunnerError::Interface)?;
        self.stats.transmitted_frames = self.stats.transmitted_frames.saturating_add(1);
        Ok(())
    }

    fn deliver(&mut self, frame: &[u8]) -> Option<Packet<MTU>> {
        let packet = match Packet::<MTU>::decode(frame) {
            Ok(packet) => packet,
            Err(_) => {
                self.stats.decode_errors = self.stats.decode_errors.saturating_add(1);
                return None;
            }
        };

        let header = packet.header();
        let is_rdp = header.flags() == Flags::RDP;
        if header.destination() != self.local_address
            && (is_rdp || header.destination() != MAX_NODE_ID)
        {
            self.stats.wrong_destination = self.stats.wrong_destination.saturating_add(1);
            return None;
        }

        if is_rdp {
            let Some(rdp) = self.rdp.as_mut() else {
                self.stats.no_socket = self.stats.no_socket.saturating_add(1);
                return None;
            };
            return match rdp.handle_packet(packet) {
                Ok(response) => response,
                Err(_) => {
                    self.stats.rdp_errors = self.stats.rdp_errors.saturating_add(1);
                    None
                }
            };
        }

        let Some(slot) = self
            .ports
            .iter()
            .position(|port| *port == Some(header.destination_port()))
        else {
            self.stats.no_socket = self.stats.no_socket.saturating_add(1);
            return None;
        };

        if let Err(TrySendError::Full(_)) = self.resources.rx[slot].try_send(packet) {
            self.stats.rx_queue_full = self.stats.rx_queue_full.saturating_add(1);
        }
        None
    }
}

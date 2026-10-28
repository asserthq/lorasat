use core::fmt;

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, TrySendError},
    signal::Signal,
};

use crate::{Error, Flags, HEADER_SIZE, Header, MAX_PORT, Packet, Priority};

pub const RDP_TRAILER_SIZE: usize = 5;
pub const RDP_SYN_OPTIONS_SIZE: usize = 24;
pub const RDP_CLIENT_PORT: u8 = MAX_PORT;

const RDP_SYN: u8 = 0x08;
const RDP_ACK: u8 = 0x04;
const RDP_EAK: u8 = 0x02;
const RDP_RST: u8 = 0x01;
const RDP_CONTROL_MASK: u8 = RDP_SYN | RDP_ACK | RDP_EAK | RDP_RST;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RdpConfig {
    initial_sequence: u16,
    connection_timeout_ms: u32,
    packet_timeout_ms: u32,
    ack_timeout_ms: u32,
    ack_delay_count: u32,
}

impl RdpConfig {
    /// Creates the first supported libcsp-compatible profile: window 1 and immediate ACKs.
    pub const fn new(initial_sequence: u16) -> Self {
        Self {
            initial_sequence,
            connection_timeout_ms: 10_000,
            packet_timeout_ms: 1_000,
            ack_timeout_ms: 250,
            ack_delay_count: 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RdpError {
    Packet(Error),
    MessageAlreadySent,
    TimedOut,
}

impl From<Error> for RdpError {
    fn from(error: Error) -> Self {
        Self::Packet(error)
    }
}

impl fmt::Display for RdpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Packet(error) => write!(formatter, "invalid RDP packet: {error}"),
            Self::MessageAlreadySent => {
                formatter.write_str("this initial RDP connection already sent its message")
            }
            Self::TimedOut => formatter.write_str("RDP operation timed out"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RdpMessage<const MTU: usize> {
    source: u16,
    source_port: u8,
    payload: [u8; MTU],
    payload_len: usize,
}

impl<const MTU: usize> RdpMessage<MTU> {
    pub const fn source(&self) -> u16 {
        self.source
    }

    pub const fn source_port(&self) -> u8 {
        self.source_port
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len]
    }
}

pub struct RdpResources<const MTU: usize> {
    commands: Channel<CriticalSectionRawMutex, RdpCommand<MTU>, 1>,
    connections: Channel<CriticalSectionRawMutex, Result<ConnectionInfo, RdpError>, 1>,
    received: Channel<CriticalSectionRawMutex, RdpMessage<MTU>, 1>,
    deliveries: Channel<CriticalSectionRawMutex, Result<(), RdpError>, 1>,
    clock: Signal<CriticalSectionRawMutex, u32>,
}

impl<const MTU: usize> RdpResources<MTU> {
    pub const fn new() -> Self {
        Self {
            commands: Channel::new(),
            connections: Channel::new(),
            received: Channel::new(),
            deliveries: Channel::new(),
            clock: Signal::new(),
        }
    }

    /// Supplies wrapping monotonic milliseconds and wakes Runner timeout processing.
    pub fn update_time(&self, now_ms: u32) {
        self.clock.signal(now_ms);
    }
}

impl<const MTU: usize> Default for RdpResources<MTU> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RdpClient<'a, const MTU: usize> {
    local_address: u16,
    resources: &'a RdpResources<MTU>,
}

impl<'a, const MTU: usize> RdpClient<'a, MTU> {
    pub(crate) const fn new(local_address: u16, resources: &'a RdpResources<MTU>) -> Self {
        Self {
            local_address,
            resources,
        }
    }

    pub async fn connect(
        self,
        destination: u16,
        destination_port: u8,
    ) -> Result<RdpConnection<'a, MTU>, RdpError> {
        Header::new(
            Priority::Normal,
            self.local_address,
            destination,
            RDP_CLIENT_PORT,
            destination_port,
            Flags::RDP,
        )?;

        self.resources
            .commands
            .send(RdpCommand::Connect {
                destination,
                destination_port,
            })
            .await;
        let info = self.resources.connections.receive().await?;
        Ok(RdpConnection::new(self.resources, info))
    }
}

pub struct RdpListener<'a, const MTU: usize> {
    resources: &'a RdpResources<MTU>,
}

impl<'a, const MTU: usize> RdpListener<'a, MTU> {
    pub(crate) const fn new(resources: &'a RdpResources<MTU>) -> Self {
        Self { resources }
    }

    /// Accepts the only connection supported by the initial RDP profile.
    pub async fn accept(self) -> Result<RdpConnection<'a, MTU>, RdpError> {
        let info = self.resources.connections.receive().await?;
        Ok(RdpConnection::new(self.resources, info))
    }
}

pub struct RdpConnection<'a, const MTU: usize> {
    resources: &'a RdpResources<MTU>,
    info: ConnectionInfo,
    sent: bool,
}

impl<'a, const MTU: usize> RdpConnection<'a, MTU> {
    const fn new(resources: &'a RdpResources<MTU>, info: ConnectionInfo) -> Self {
        Self {
            resources,
            info,
            sent: false,
        }
    }

    pub const fn peer_address(&self) -> u16 {
        self.info.peer_address
    }

    pub const fn local_port(&self) -> u8 {
        self.info.local_port
    }

    pub const fn peer_port(&self) -> u8 {
        self.info.peer_port
    }

    /// Sends the one DATA message supported by the initial profile and waits for its RDP ACK.
    pub async fn send(&mut self, payload: &[u8]) -> Result<(), RdpError> {
        if self.sent {
            return Err(RdpError::MessageAlreadySent);
        }

        let packet_len = HEADER_SIZE + payload.len() + RDP_TRAILER_SIZE;
        if packet_len > MTU {
            return Err(RdpError::Packet(Error::PacketTooLarge {
                maximum: MTU,
                actual: packet_len,
            }));
        }

        let mut owned_payload = [0; MTU];
        owned_payload[..payload.len()].copy_from_slice(payload);
        self.sent = true;
        self.resources
            .commands
            .send(RdpCommand::Send {
                payload: owned_payload,
                payload_len: payload.len(),
            })
            .await;
        self.resources.deliveries.receive().await
    }

    pub async fn recv(&mut self) -> RdpMessage<MTU> {
        self.resources.received.receive().await
    }

    pub fn try_recv(&mut self) -> Option<RdpMessage<MTU>> {
        self.resources.received.try_receive().ok()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum RdpBinding {
    Client(RdpConfig),
    Server { port: u8, config: RdpConfig },
}

pub(crate) enum RdpCommand<const MTU: usize> {
    Connect {
        destination: u16,
        destination_port: u8,
    },
    Send {
        payload: [u8; MTU],
        payload_len: usize,
    },
}

#[derive(Clone, Copy)]
struct ConnectionInfo {
    peer_address: u16,
    local_port: u8,
    peer_port: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RdpHeader {
    flags: u8,
    sequence: u16,
    acknowledgement: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SynOptions {
    window_size: u32,
    connection_timeout_ms: u32,
    packet_timeout_ms: u32,
    delayed_acks: u32,
    ack_timeout_ms: u32,
    ack_delay_count: u32,
}

impl From<RdpConfig> for SynOptions {
    fn from(config: RdpConfig) -> Self {
        Self {
            window_size: 1,
            connection_timeout_ms: config.connection_timeout_ms,
            packet_timeout_ms: config.packet_timeout_ms,
            delayed_acks: 0,
            ack_timeout_ms: config.ack_timeout_ms,
            ack_delay_count: config.ack_delay_count,
        }
    }
}

#[derive(Clone, Copy)]
enum EngineState {
    ClientClosed {
        config: RdpConfig,
    },
    ClientSynSent {
        config: RdpConfig,
        peer_address: u16,
        peer_port: u8,
        send_initial: u16,
        send_next: u16,
    },
    ServerClosed {
        port: u8,
        config: RdpConfig,
    },
    ServerSynReceived {
        port: u8,
        config: RdpConfig,
        peer_address: u16,
        peer_port: u8,
        send_initial: u16,
        send_next: u16,
        receive_initial: u16,
    },
    Open {
        peer_address: u16,
        local_port: u8,
        peer_port: u8,
        send_next: u16,
        receive_current: u16,
        pending_sequence: Option<u16>,
    },
}

#[derive(Clone, Copy)]
enum PendingKind {
    Handshake,
    Data,
}

struct PendingPacket<const MTU: usize> {
    packet: Packet<MTU>,
    kind: PendingKind,
    first_sent_at: u32,
    last_sent_at: u32,
}

pub(crate) struct RdpEngine<'a, const MTU: usize> {
    local_address: u16,
    resources: &'a RdpResources<MTU>,
    config: RdpConfig,
    state: EngineState,
    pending: Option<PendingPacket<MTU>>,
    now_ms: u32,
    control_nonce: u8,
}

impl<'a, const MTU: usize> RdpEngine<'a, MTU> {
    pub(crate) const fn new(
        local_address: u16,
        resources: &'a RdpResources<MTU>,
        binding: RdpBinding,
    ) -> Self {
        let (state, config) = match binding {
            RdpBinding::Client(config) => (EngineState::ClientClosed { config }, config),
            RdpBinding::Server { port, config } => {
                (EngineState::ServerClosed { port, config }, config)
            }
        };
        Self {
            local_address,
            resources,
            config,
            state,
            pending: None,
            now_ms: 0,
            control_nonce: 0,
        }
    }

    pub(crate) async fn next_command(&self) -> RdpCommand<MTU> {
        self.resources.commands.receive().await
    }

    pub(crate) async fn next_time_update(&self) -> u32 {
        self.resources.clock.wait().await
    }

    pub(crate) fn handle_time_update(&mut self, now_ms: u32) -> Option<Packet<MTU>> {
        self.now_ms = now_ms;
        let pending = self.pending.as_mut()?;

        if time_after(
            now_ms,
            pending
                .first_sent_at
                .wrapping_add(self.config.connection_timeout_ms),
        ) {
            let kind = pending.kind;
            self.pending = None;
            match self.state {
                EngineState::ClientSynSent { config, .. } => {
                    self.state = EngineState::ClientClosed { config };
                    let _ = self.resources.connections.try_send(Err(RdpError::TimedOut));
                }
                EngineState::ServerSynReceived { port, config, .. } => {
                    self.state = EngineState::ServerClosed { port, config };
                }
                EngineState::Open {
                    peer_address,
                    local_port,
                    peer_port,
                    send_next,
                    receive_current,
                    ..
                } => {
                    self.state = EngineState::Open {
                        peer_address,
                        local_port,
                        peer_port,
                        send_next,
                        receive_current,
                        pending_sequence: None,
                    };
                    if matches!(kind, PendingKind::Data) {
                        let _ = self.resources.deliveries.try_send(Err(RdpError::TimedOut));
                    }
                }
                EngineState::ClientClosed { .. } | EngineState::ServerClosed { .. } => {}
            }
            return None;
        }

        if time_after(
            now_ms,
            pending
                .last_sent_at
                .wrapping_add(self.config.packet_timeout_ms),
        ) {
            pending.last_sent_at = now_ms;
            if matches!(pending.kind, PendingKind::Data)
                && let EngineState::Open {
                    receive_current, ..
                } = self.state
            {
                let payload = pending.packet.payload_mut();
                let acknowledgement = payload.len() - 2;
                payload[acknowledgement..].copy_from_slice(&receive_current.to_be_bytes());
            }
            return Some(pending.packet.clone());
        }
        None
    }

    pub(crate) fn handle_command(
        &mut self,
        command: RdpCommand<MTU>,
    ) -> Result<Option<Packet<MTU>>, Error> {
        match (self.state, command) {
            (
                EngineState::ClientClosed { config },
                RdpCommand::Connect {
                    destination,
                    destination_port,
                },
            ) => {
                let send_initial = config.initial_sequence;
                let send_next = send_initial.wrapping_add(1);
                self.state = EngineState::ClientSynSent {
                    config,
                    peer_address: destination,
                    peer_port: destination_port,
                    send_initial,
                    send_next,
                };

                let mut options = [0; RDP_SYN_OPTIONS_SIZE];
                encode_syn_options(SynOptions::from(config), &mut options)?;
                let flags = self.control_flags(RDP_SYN);
                let packet = self.make_packet(
                    destination,
                    RDP_CLIENT_PORT,
                    destination_port,
                    Priority::High,
                    &options,
                    RdpHeader {
                        flags,
                        sequence: send_initial,
                        acknowledgement: 0,
                    },
                )?;
                self.track(packet.clone(), PendingKind::Handshake);
                Ok(Some(packet))
            }
            (
                EngineState::Open {
                    peer_address,
                    local_port,
                    peer_port,
                    send_next,
                    receive_current,
                    pending_sequence: None,
                },
                RdpCommand::Send {
                    payload,
                    payload_len,
                },
            ) => {
                self.state = EngineState::Open {
                    peer_address,
                    local_port,
                    peer_port,
                    send_next: send_next.wrapping_add(1),
                    receive_current,
                    pending_sequence: Some(send_next),
                };
                let packet = self.make_packet(
                    peer_address,
                    local_port,
                    peer_port,
                    Priority::Normal,
                    &payload[..payload_len],
                    RdpHeader {
                        flags: RDP_ACK,
                        sequence: send_next,
                        acknowledgement: receive_current,
                    },
                )?;
                self.track(packet.clone(), PendingKind::Data);
                Ok(Some(packet))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn handle_packet(
        &mut self,
        packet: Packet<MTU>,
    ) -> Result<Option<Packet<MTU>>, Error> {
        let packet_header = packet.header();
        let (data, rdp_header) = decode_segment(packet.payload())?;
        let control = rdp_header.flags & RDP_CONTROL_MASK;

        match self.state {
            EngineState::ClientSynSent {
                config,
                peer_address,
                peer_port,
                send_initial,
                send_next,
            } => {
                if !matches_packet(packet_header, peer_address, peer_port, RDP_CLIENT_PORT)
                    || control != (RDP_SYN | RDP_ACK)
                    || !data.is_empty()
                    || rdp_header.acknowledgement != send_initial
                {
                    return Ok(None);
                }

                let receive_current = rdp_header.sequence;
                self.pending = None;
                self.state = EngineState::Open {
                    peer_address,
                    local_port: RDP_CLIENT_PORT,
                    peer_port,
                    send_next,
                    receive_current,
                    pending_sequence: None,
                };
                let _ = config;
                let _ = self.resources.connections.try_send(Ok(ConnectionInfo {
                    peer_address,
                    local_port: RDP_CLIENT_PORT,
                    peer_port,
                }));

                let flags = self.control_flags(RDP_ACK);
                self.make_packet(
                    peer_address,
                    RDP_CLIENT_PORT,
                    peer_port,
                    Priority::High,
                    &[],
                    RdpHeader {
                        flags,
                        sequence: send_next,
                        acknowledgement: receive_current,
                    },
                )
                .map(Some)
            }
            EngineState::ServerClosed { port, config } => {
                if packet_header.destination_port() != port || control != RDP_SYN {
                    return Ok(None);
                }
                let options = decode_syn_options(data)?;
                if options.window_size != 1 || options.delayed_acks != 0 {
                    return Ok(None);
                }

                let send_initial = config.initial_sequence;
                let send_next = send_initial.wrapping_add(1);
                let receive_initial = rdp_header.sequence;
                let peer_address = packet_header.source();
                let peer_port = packet_header.source_port();
                self.state = EngineState::ServerSynReceived {
                    port,
                    config,
                    peer_address,
                    peer_port,
                    send_initial,
                    send_next,
                    receive_initial,
                };

                let flags = self.control_flags(RDP_SYN | RDP_ACK);
                let packet = self.make_packet(
                    peer_address,
                    port,
                    peer_port,
                    Priority::High,
                    &[],
                    RdpHeader {
                        flags,
                        sequence: send_initial,
                        acknowledgement: receive_initial,
                    },
                )?;
                self.track(packet.clone(), PendingKind::Handshake);
                Ok(Some(packet))
            }
            EngineState::ServerSynReceived {
                port,
                config,
                peer_address,
                peer_port,
                send_initial,
                send_next,
                receive_initial,
            } => {
                if matches_packet(packet_header, peer_address, peer_port, port)
                    && control == RDP_SYN
                    && rdp_header.sequence == receive_initial
                {
                    return Ok(self.pending.as_ref().map(|pending| pending.packet.clone()));
                }
                if !matches_packet(packet_header, peer_address, peer_port, port)
                    || control != RDP_ACK
                    || rdp_header.sequence != receive_initial.wrapping_add(1)
                    || rdp_header.acknowledgement != send_initial
                {
                    return Ok(None);
                }

                self.pending = None;
                let mut receive_current = receive_initial;
                let received_data = if data.is_empty() {
                    false
                } else {
                    let mut payload = [0; MTU];
                    payload[..data.len()].copy_from_slice(data);
                    if self
                        .resources
                        .received
                        .try_send(RdpMessage {
                            source: peer_address,
                            source_port: peer_port,
                            payload,
                            payload_len: data.len(),
                        })
                        .is_ok()
                    {
                        receive_current = rdp_header.sequence;
                        true
                    } else {
                        false
                    }
                };
                self.state = EngineState::Open {
                    peer_address,
                    local_port: port,
                    peer_port,
                    send_next,
                    receive_current,
                    pending_sequence: None,
                };
                let _ = config;
                let _ = self.resources.connections.try_send(Ok(ConnectionInfo {
                    peer_address,
                    local_port: port,
                    peer_port,
                }));
                if received_data {
                    self.acknowledgement_packet(
                        peer_address,
                        port,
                        peer_port,
                        send_next,
                        receive_current,
                    )
                } else {
                    Ok(None)
                }
            }
            EngineState::Open {
                peer_address,
                local_port,
                peer_port,
                send_next,
                mut receive_current,
                mut pending_sequence,
            } => {
                if matches_packet(packet_header, peer_address, peer_port, local_port)
                    && control == (RDP_SYN | RDP_ACK)
                    && data.is_empty()
                    && rdp_header.sequence == receive_current
                {
                    return self.acknowledgement_packet(
                        peer_address,
                        local_port,
                        peer_port,
                        send_next,
                        receive_current,
                    );
                }
                if !matches_packet(packet_header, peer_address, peer_port, local_port)
                    || control & RDP_ACK == 0
                    || control & (RDP_SYN | RDP_RST) != 0
                {
                    return Ok(None);
                }

                let expected = receive_current.wrapping_add(1);
                if !data.is_empty() && rdp_header.sequence == receive_current {
                    return self.acknowledgement_packet(
                        peer_address,
                        local_port,
                        peer_port,
                        send_next,
                        receive_current,
                    );
                }
                if !sequence_between(rdp_header.sequence, expected, expected.wrapping_add(1)) {
                    return Ok(None);
                }

                let send_unacknowledged = pending_sequence.unwrap_or(send_next);
                if !sequence_between(
                    rdp_header.acknowledgement,
                    send_unacknowledged.wrapping_sub(3),
                    send_next.wrapping_sub(1),
                ) {
                    return Ok(None);
                }

                if pending_sequence == Some(rdp_header.acknowledgement) {
                    pending_sequence = None;
                    if matches!(
                        self.pending.take().map(|pending| pending.kind),
                        Some(PendingKind::Data)
                    ) {
                        let _ = self.resources.deliveries.try_send(Ok(()));
                    }
                }

                if data.is_empty() || rdp_header.sequence != expected {
                    self.state = EngineState::Open {
                        peer_address,
                        local_port,
                        peer_port,
                        send_next,
                        receive_current,
                        pending_sequence,
                    };
                    return Ok(None);
                }

                let mut payload = [0; MTU];
                payload[..data.len()].copy_from_slice(data);
                match self.resources.received.try_send(RdpMessage {
                    source: peer_address,
                    source_port: peer_port,
                    payload,
                    payload_len: data.len(),
                }) {
                    Ok(()) => {
                        receive_current = rdp_header.sequence;
                        self.state = EngineState::Open {
                            peer_address,
                            local_port,
                            peer_port,
                            send_next,
                            receive_current,
                            pending_sequence,
                        };
                        self.acknowledgement_packet(
                            peer_address,
                            local_port,
                            peer_port,
                            send_next,
                            receive_current,
                        )
                    }
                    Err(TrySendError::Full(_)) => Ok(None),
                }
            }
            EngineState::ClientClosed { .. } => Ok(None),
        }
    }

    fn acknowledgement_packet(
        &mut self,
        peer_address: u16,
        local_port: u8,
        peer_port: u8,
        send_next: u16,
        receive_current: u16,
    ) -> Result<Option<Packet<MTU>>, Error> {
        let flags = self.control_flags(RDP_ACK);
        self.make_packet(
            peer_address,
            local_port,
            peer_port,
            Priority::High,
            &[],
            RdpHeader {
                flags,
                sequence: send_next,
                acknowledgement: receive_current,
            },
        )
        .map(Some)
    }

    fn track(&mut self, packet: Packet<MTU>, kind: PendingKind) {
        self.pending = Some(PendingPacket {
            packet,
            kind,
            first_sent_at: self.now_ms,
            last_sent_at: self.now_ms,
        });
    }

    fn control_flags(&mut self, control: u8) -> u8 {
        let flags = (self.control_nonce << 4) | control;
        self.control_nonce = self.control_nonce.wrapping_add(1);
        flags
    }

    fn make_packet(
        &self,
        peer_address: u16,
        local_port: u8,
        peer_port: u8,
        priority: Priority,
        data: &[u8],
        rdp_header: RdpHeader,
    ) -> Result<Packet<MTU>, Error> {
        let header = Header::new(
            priority,
            self.local_address,
            peer_address,
            local_port,
            peer_port,
            Flags::RDP,
        )?;
        let mut payload = [0; MTU];
        let payload_len = encode_segment(data, rdp_header, &mut payload)?;
        Packet::new(header, &payload[..payload_len])
    }
}

fn sequence_between(sequence: u16, start: u16, end: u16) -> bool {
    end.wrapping_sub(start) >= sequence.wrapping_sub(start)
}

fn time_after(time: u32, compare: u32) -> bool {
    (time.wrapping_sub(compare) as i32) > 0
}

fn matches_packet(header: Header, peer_address: u16, peer_port: u8, local_port: u8) -> bool {
    header.flags() == Flags::RDP
        && header.source() == peer_address
        && header.source_port() == peer_port
        && header.destination_port() == local_port
}

fn encode_segment(data: &[u8], header: RdpHeader, output: &mut [u8]) -> Result<usize, Error> {
    let required = data.len() + RDP_TRAILER_SIZE;
    if output.len() < required {
        return Err(Error::OutputTooShort {
            required,
            actual: output.len(),
        });
    }
    output[..data.len()].copy_from_slice(data);
    output[data.len()] = header.flags;
    output[data.len() + 1..data.len() + 3].copy_from_slice(&header.sequence.to_be_bytes());
    output[data.len() + 3..required].copy_from_slice(&header.acknowledgement.to_be_bytes());
    Ok(required)
}

fn decode_segment(input: &[u8]) -> Result<(&[u8], RdpHeader), Error> {
    if input.len() < RDP_TRAILER_SIZE {
        return Err(Error::InputTooShort {
            required: RDP_TRAILER_SIZE,
            actual: input.len(),
        });
    }
    let trailer = input.len() - RDP_TRAILER_SIZE;
    Ok((
        &input[..trailer],
        RdpHeader {
            flags: input[trailer],
            sequence: u16::from_be_bytes([input[trailer + 1], input[trailer + 2]]),
            acknowledgement: u16::from_be_bytes([input[trailer + 3], input[trailer + 4]]),
        },
    ))
}

fn encode_syn_options(options: SynOptions, output: &mut [u8]) -> Result<usize, Error> {
    if output.len() < RDP_SYN_OPTIONS_SIZE {
        return Err(Error::OutputTooShort {
            required: RDP_SYN_OPTIONS_SIZE,
            actual: output.len(),
        });
    }
    let values = [
        options.window_size,
        options.connection_timeout_ms,
        options.packet_timeout_ms,
        options.delayed_acks,
        options.ack_timeout_ms,
        options.ack_delay_count,
    ];
    for (index, value) in values.into_iter().enumerate() {
        let start = index * 4;
        output[start..start + 4].copy_from_slice(&value.to_be_bytes());
    }
    Ok(RDP_SYN_OPTIONS_SIZE)
}

fn decode_syn_options(input: &[u8]) -> Result<SynOptions, Error> {
    if input.len() < RDP_SYN_OPTIONS_SIZE {
        return Err(Error::InputTooShort {
            required: RDP_SYN_OPTIONS_SIZE,
            actual: input.len(),
        });
    }
    let value = |index: usize| {
        let start = index * 4;
        u32::from_be_bytes([
            input[start],
            input[start + 1],
            input[start + 2],
            input[start + 3],
        ])
    };
    Ok(SynOptions {
        window_size: value(0),
        connection_timeout_ms: value(1),
        packet_timeout_ms: value(2),
        delayed_acks: value(3),
        ack_timeout_ms: value(4),
        ack_delay_count: value(5),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdp_trailer_matches_libcsp_layout() {
        let mut encoded = [0; 8];
        let length = encode_segment(
            b"abc",
            RdpHeader {
                flags: RDP_SYN | RDP_ACK,
                sequence: 0x1234,
                acknowledgement: 0xABCD,
            },
            &mut encoded,
        )
        .unwrap();

        assert_eq!(length, 8);
        assert_eq!(encoded, [b'a', b'b', b'c', 0x0C, 0x12, 0x34, 0xAB, 0xCD]);
        assert_eq!(
            decode_segment(&encoded).unwrap(),
            (
                b"abc".as_slice(),
                RdpHeader {
                    flags: 0x0C,
                    sequence: 0x1234,
                    acknowledgement: 0xABCD,
                }
            )
        );
    }

    #[test]
    fn syn_options_match_libcsp_layout() {
        let config = RdpConfig::new(0x1234);
        let mut encoded = [0; RDP_SYN_OPTIONS_SIZE];
        encode_syn_options(SynOptions::from(config), &mut encoded).unwrap();

        assert_eq!(
            encoded,
            [
                0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x27, 0x10, 0x00, 0x00, 0x03, 0xE8, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0xFA, 0x00, 0x00, 0x00, 0x02,
            ]
        );
        assert_eq!(
            decode_syn_options(&encoded).unwrap(),
            SynOptions::from(config)
        );
    }
}

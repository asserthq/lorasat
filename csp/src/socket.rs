use core::fmt;

use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Receiver, Sender, TrySendError},
};

use crate::{Error, Flags, Header, Packet, Priority};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindError {
    InvalidPort(u8),
    PortInUse(u8),
    NoSocketSlots,
    RdpAlreadyConfigured,
    RdpMtuTooSmall { minimum: usize, actual: usize },
}

impl fmt::Display for BindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPort(port) => write!(formatter, "port {port} exceeds 6 bits"),
            Self::PortInUse(port) => write!(formatter, "port {port} is already bound"),
            Self::NoSocketSlots => formatter.write_str("no free socket slots"),
            Self::RdpAlreadyConfigured => {
                formatter.write_str("an RDP client or listener is already configured")
            }
            Self::RdpMtuTooSmall { minimum, actual } => {
                write!(formatter, "RDP needs MTU {minimum}, got {actual}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendError {
    Packet(Error),
    Busy,
}

impl From<Error> for SendError {
    fn from(error: Error) -> Self {
        Self::Packet(error)
    }
}

impl fmt::Display for SendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Packet(error) => write!(formatter, "invalid packet: {error}"),
            Self::Busy => formatter.write_str("transmit queue is full"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Datagram<const MTU: usize> {
    packet: Packet<MTU>,
}

impl<const MTU: usize> Datagram<MTU> {
    pub(crate) const fn new(packet: Packet<MTU>) -> Self {
        Self { packet }
    }

    pub const fn source(&self) -> u16 {
        self.packet.header().source()
    }

    pub const fn source_port(&self) -> u8 {
        self.packet.header().source_port()
    }

    pub const fn destination(&self) -> u16 {
        self.packet.header().destination()
    }

    pub const fn destination_port(&self) -> u8 {
        self.packet.header().destination_port()
    }

    pub fn payload(&self) -> &[u8] {
        self.packet.payload()
    }
}

pub struct Socket<'a, const MTU: usize, const TX_DEPTH: usize, const RX_DEPTH: usize> {
    local_address: u16,
    local_port: u8,
    tx: Sender<'a, CriticalSectionRawMutex, Packet<MTU>, TX_DEPTH>,
    rx: Receiver<'a, CriticalSectionRawMutex, Packet<MTU>, RX_DEPTH>,
}

impl<'a, const MTU: usize, const TX_DEPTH: usize, const RX_DEPTH: usize>
    Socket<'a, MTU, TX_DEPTH, RX_DEPTH>
{
    pub(crate) const fn new(
        local_address: u16,
        local_port: u8,
        tx: Sender<'a, CriticalSectionRawMutex, Packet<MTU>, TX_DEPTH>,
        rx: Receiver<'a, CriticalSectionRawMutex, Packet<MTU>, RX_DEPTH>,
    ) -> Self {
        Self {
            local_address,
            local_port,
            tx,
            rx,
        }
    }

    pub const fn local_address(&self) -> u16 {
        self.local_address
    }

    pub const fn local_port(&self) -> u8 {
        self.local_port
    }

    pub async fn send_to(
        &self,
        destination: u16,
        destination_port: u8,
        payload: &[u8],
    ) -> Result<(), SendError> {
        let packet = self.packet(destination, destination_port, payload)?;
        self.tx.send(packet).await;
        Ok(())
    }

    pub fn try_send_to(
        &self,
        destination: u16,
        destination_port: u8,
        payload: &[u8],
    ) -> Result<(), SendError> {
        let packet = self.packet(destination, destination_port, payload)?;
        self.tx.try_send(packet).map_err(|error| match error {
            TrySendError::Full(_) => SendError::Busy,
        })
    }

    /// Waits for one datagram. Mutable access enforces one receive waiter.
    pub async fn recv_from(&mut self) -> Datagram<MTU> {
        Datagram::new(self.rx.receive().await)
    }

    /// Receives one queued datagram without waiting.
    pub fn try_recv_from(&mut self) -> Option<Datagram<MTU>> {
        self.rx.try_receive().ok().map(Datagram::new)
    }

    fn packet(
        &self,
        destination: u16,
        destination_port: u8,
        payload: &[u8],
    ) -> Result<Packet<MTU>, SendError> {
        let header = Header::new(
            Priority::Normal,
            self.local_address,
            destination,
            self.local_port,
            destination_port,
            Flags::NONE,
        )?;
        Ok(Packet::new(header, payload)?)
    }
}

use core::fmt;

use sat_csp::{BindError, CspInterface, Error, RdpConfig, Runner, RunnerBuilder};
use sat_drivers::csp::{CspDatagramSocket, CspRdpListener};

use crate::{
    config::network::{
        BEACON_PORT, COMMAND_PORT, MTU, RX_QUEUE_DEPTH, SATELLITE_ADDRESS, SOCKET_COUNT,
        TX_QUEUE_DEPTH,
    },
    resources::ProtocolStackResources,
};

pub(crate) type BeaconSocket<'a> = CspDatagramSocket<'a, MTU, TX_QUEUE_DEPTH, RX_QUEUE_DEPTH>;
pub(crate) type CommandListener<'a> = CspRdpListener<'a, MTU>;
pub(crate) type CspRunner<'a, I> = Runner<'a, I, MTU, TX_QUEUE_DEPTH, SOCKET_COUNT, RX_QUEUE_DEPTH>;

#[derive(Debug)]
pub(crate) enum NetworkInitError {
    Stack(Error),
    Bind(BindError),
}

impl fmt::Display for NetworkInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stack(error) => write!(formatter, "invalid CSP stack: {error}"),
            Self::Bind(error) => write!(formatter, "failed to bind CSP endpoint: {error}"),
        }
    }
}

pub(crate) struct CspStack<'a, I> {
    pub beacon_socket: BeaconSocket<'a>,
    pub command_listener: CommandListener<'a>,
    pub runner: CspRunner<'a, I>,
}

impl<'a, I> CspStack<'a, I>
where
    I: CspInterface,
{
    pub fn bind(
        interface: I,
        resources: &'a ProtocolStackResources,
    ) -> Result<Self, NetworkInitError> {
        resources.rdp.update_time(0);

        let mut builder = RunnerBuilder::new(SATELLITE_ADDRESS.get(), interface, &resources.stack)
            .map_err(NetworkInitError::Stack)?;

        let command_listener = CspRdpListener::new(
            builder
                .listen_rdp(COMMAND_PORT, &resources.rdp, RdpConfig::new(0x2000))
                .map_err(NetworkInitError::Bind)?,
        );
        let beacon_socket =
            CspDatagramSocket::new(builder.bind(BEACON_PORT).map_err(NetworkInitError::Bind)?);

        Ok(Self {
            beacon_socket,
            command_listener,
            runner: builder.build(),
        })
    }
}

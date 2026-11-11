use host_sil::{
    config::{BEACON_PORT, CMD_PORT, GS_ADDR, MTU, SAT_ADDR},
    csp_demo::{CspDatagramSocket, CspRdpListener},
};
use sat_csp::{RdpConfig, RdpResources, Runner, RunnerBuilder, StackResources};
use test_utils::host::{RadioBand, UdpCspInterface};

const TX_QUEUE_DEPTH: usize = 4;
const SOCKET_COUNT: usize = 1;
const RX_QUEUE_DEPTH: usize = 4;

pub type CspResources = StackResources<MTU, TX_QUEUE_DEPTH, SOCKET_COUNT, RX_QUEUE_DEPTH>;
pub type ReliableResources = RdpResources<MTU>;

type BeaconSocket<'a> = CspDatagramSocket<'a, MTU, TX_QUEUE_DEPTH, RX_QUEUE_DEPTH>;
type CommandListener<'a> = CspRdpListener<'a, MTU>;
type CspRunner<'a> = Runner<'a, UdpCspInterface, MTU, TX_QUEUE_DEPTH, SOCKET_COUNT, RX_QUEUE_DEPTH>;

pub struct CspStack<'a> {
    pub beacon_socket: BeaconSocket<'a>,
    pub command_listener: CommandListener<'a>,
    pub runner: CspRunner<'a>,
}

impl<'a> CspStack<'a> {
    pub async fn bind(
        resources: &'a CspResources,
        reliable_resources: &'a ReliableResources,
    ) -> Self {
        let interface = UdpCspInterface::bind(RadioBand::Mhz435, SAT_ADDR.get(), &[GS_ADDR.get()])
            .await
            .unwrap();
        let mut builder = RunnerBuilder::new(SAT_ADDR.get(), interface, resources).unwrap();
        let command_listener = CspRdpListener::new(
            builder
                .listen_rdp(CMD_PORT, reliable_resources, RdpConfig::new(0x2000))
                .unwrap(),
        );
        let beacon_socket = CspDatagramSocket::new(builder.bind(BEACON_PORT).unwrap());

        Self {
            beacon_socket,
            command_listener,
            runner: builder.build(),
        }
    }
}

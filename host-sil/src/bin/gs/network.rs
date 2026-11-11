use examples_std::{
    csp_demo::CspDatagramSocket,
    peers::{BEACON_PORT, GS_ADDR, MTU, SAT_ADDR},
};
use sat_csp::{RdpClient, RdpConfig, RdpResources, Runner, RunnerBuilder, StackResources};
use test_utils::host::{RadioBand, UdpCspInterface};

const TX_QUEUE_DEPTH: usize = 4;
const SOCKET_COUNT: usize = 1;
const RX_QUEUE_DEPTH: usize = 4;

pub type CspResources = StackResources<MTU, TX_QUEUE_DEPTH, SOCKET_COUNT, RX_QUEUE_DEPTH>;
pub type ReliableResources = RdpResources<MTU>;

type BeaconSocket<'a> = CspDatagramSocket<'a, MTU, TX_QUEUE_DEPTH, RX_QUEUE_DEPTH>;
type CommandClient<'a> = RdpClient<'a, MTU>;
type CspRunner<'a> = Runner<'a, UdpCspInterface, MTU, TX_QUEUE_DEPTH, SOCKET_COUNT, RX_QUEUE_DEPTH>;

pub struct CspStack<'a> {
    pub beacon_socket: BeaconSocket<'a>,
    pub command_client: CommandClient<'a>,
    pub runner: CspRunner<'a>,
}

impl<'a> CspStack<'a> {
    pub async fn bind(
        resources: &'a CspResources,
        reliable_resources: &'a ReliableResources,
    ) -> Self {
        let interface = UdpCspInterface::bind(RadioBand::Mhz435, GS_ADDR.get(), &[SAT_ADDR.get()])
            .await
            .unwrap();
        let mut builder = RunnerBuilder::new(GS_ADDR.get(), interface, resources).unwrap();
        let command_client = builder
            .rdp_client(reliable_resources, RdpConfig::new(0x1000))
            .unwrap();
        let beacon_socket = CspDatagramSocket::new(builder.bind(BEACON_PORT).unwrap());

        Self {
            beacon_socket,
            command_client,
            runner: builder.build(),
        }
    }
}

use host_sil::config::{BEACON_PORT, CLIENT_ADDR, CLIENT_DATA_PORT, MTU, SAT_ADDR};
use sat_csp::{Runner, RunnerBuilder, Socket, StackResources};
use sat_drivers::csp::CspDatagramSocket;
use test_utils::host::{RadioBand, UdpCspInterface};

const TX_QUEUE_DEPTH: usize = 4;
const SOCKET_COUNT: usize = 2;
const RX_QUEUE_DEPTH: usize = 4;

pub type CspResources = StackResources<MTU, TX_QUEUE_DEPTH, SOCKET_COUNT, RX_QUEUE_DEPTH>;

type DatagramSocket<'a> = CspDatagramSocket<'a, MTU, TX_QUEUE_DEPTH, RX_QUEUE_DEPTH>;
type DataSocket<'a> = Socket<'a, MTU, TX_QUEUE_DEPTH, RX_QUEUE_DEPTH>;
type CspRunner<'a> = Runner<'a, UdpCspInterface, MTU, TX_QUEUE_DEPTH, SOCKET_COUNT, RX_QUEUE_DEPTH>;

pub struct CspStack<'a> {
    pub beacon_socket: DatagramSocket<'a>,
    pub data_socket: DataSocket<'a>,
    pub runner: CspRunner<'a>,
}

impl<'a> CspStack<'a> {
    pub async fn bind(resources: &'a CspResources) -> Self {
        let interface =
            UdpCspInterface::bind(RadioBand::Mhz868, CLIENT_ADDR.get(), &[SAT_ADDR.get()])
                .await
                .expect("ASP could not bind its 868 MHz host-SIL UDP interface");
        let mut builder = RunnerBuilder::new(CLIENT_ADDR.get(), interface, resources)
            .expect("ASP CSP stack configuration is invalid");
        let beacon_socket = CspDatagramSocket::new(
            builder
                .bind(BEACON_PORT)
                .expect("ASP could not bind the beacon port"),
        );
        let data_socket = builder
            .bind(CLIENT_DATA_PORT)
            .expect("ASP could not bind the client data port");

        Self {
            beacon_socket,
            data_socket,
            runner: builder.build(),
        }
    }
}

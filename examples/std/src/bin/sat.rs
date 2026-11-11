use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, pubsub::PubSubChannel,
};
use embassy_time::Duration;
use examples_std::peers::{BEACON_INTERVAL_SECS, BEACON_PORT, CMD_PORT, GS_ADDR, SAT_ADDR};
use sat_core::{
    adcs::{AdcsCommand, AdcsEvent},
    eps::{EpsCommand, EpsEvent},
    net::Endpoint,
};

use sat_drivers::storage::RamStorage;
use sat_subsystems::cdh::{CdhConfig, CdhSystem, SubsystemLink, SubsystemsBundle};
use test_utils::host::net::{TcpSessionListener, UdpDatagramSocket};

const COMMAND_QUEUE_DEPTH: usize = 4;
const EVENT_QUEUE_DEPTH: usize = 4;
const EVENT_SUBSCRIBERS: usize = 4;
const EVENT_PUBLISHERS: usize = 4;

const STORAGE_ENTRY_SIZE: usize = 300;
const STORAGE_DEPTH: usize = 16;

type AdcsCommandChannel = Channel<CriticalSectionRawMutex, AdcsCommand, COMMAND_QUEUE_DEPTH>;
type AdcsEventChannel = PubSubChannel<
    CriticalSectionRawMutex,
    AdcsEvent,
    EVENT_QUEUE_DEPTH,
    EVENT_SUBSCRIBERS,
    EVENT_PUBLISHERS,
>;
type EpsCommandChannel = Channel<CriticalSectionRawMutex, EpsCommand, COMMAND_QUEUE_DEPTH>;
type EpsEventChannel = PubSubChannel<
    CriticalSectionRawMutex,
    EpsEvent,
    EVENT_QUEUE_DEPTH,
    EVENT_SUBSCRIBERS,
    EVENT_PUBLISHERS,
>;

static ADCS_COMMANDS: AdcsCommandChannel = AdcsCommandChannel::new();
static ADCS_EVENTS: AdcsEventChannel = AdcsEventChannel::new();
static EPS_COMMANDS: EpsCommandChannel = EpsCommandChannel::new();
static EPS_EVENTS: EpsEventChannel = EpsEventChannel::new();

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let beacon_socket = UdpDatagramSocket::bind(Endpoint::new(SAT_ADDR, BEACON_PORT), &[GS_ADDR])
        .await
        .unwrap();
    let ground_listener = TcpSessionListener::bind(Endpoint::new(SAT_ADDR, CMD_PORT))
        .await
        .unwrap();

    let subsystems = SubsystemsBundle {
        adcs: SubsystemLink {
            cmd_tx: ADCS_COMMANDS.sender(),
            event_rx: ADCS_EVENTS.subscriber().unwrap(),
        },
        eps: SubsystemLink {
            cmd_tx: EPS_COMMANDS.sender(),
            event_rx: EPS_EVENTS.subscriber().unwrap(),
        },
    };

    // TODO: передать эти handles в реальные AdcsSystem и EpsSystem.
    let _adcs_handles = (ADCS_COMMANDS.receiver(), ADCS_EVENTS.publisher().unwrap());
    let _eps_handles = (EPS_COMMANDS.receiver(), EPS_EVENTS.publisher().unwrap());

    let cdh_config = CdhConfig {
        beacon_interval: Duration::from_secs(u64::from(BEACON_INTERVAL_SECS)),
        satellite_address: SAT_ADDR,
        beacon_port: BEACON_PORT,
    };
    let platform_storage = RamStorage::<STORAGE_ENTRY_SIZE, STORAGE_DEPTH>::new();
    let iot_storage = RamStorage::<STORAGE_ENTRY_SIZE, STORAGE_DEPTH>::new();

    let mut cdh = CdhSystem::new(
        cdh_config,
        subsystems,
        beacon_socket,
        ground_listener,
        platform_storage,
        iot_storage,
    );

    cdh.run().await;
}

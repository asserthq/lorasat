use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, pubsub::PubSubChannel,
};
use sat_core::{
    adcs::{AdcsCommand, AdcsEvent},
    eps::{EpsCommand, EpsEvent},
};
use sat_subsystems::cdh::{SubsystemLink, SubsystemsBundle};

const COMMAND_QUEUE_DEPTH: usize = 4;
const EVENT_QUEUE_DEPTH: usize = 4;
const EVENT_SUBSCRIBERS: usize = 4;
const EVENT_PUBLISHERS: usize = 4;

type GenericCommandChannel<C> = Channel<CriticalSectionRawMutex, C, COMMAND_QUEUE_DEPTH>;
type GenericEventChannel<E> = PubSubChannel<
    CriticalSectionRawMutex,
    E,
    EVENT_QUEUE_DEPTH,
    EVENT_SUBSCRIBERS,
    EVENT_PUBLISHERS,
>;

type AdcsCommandChannel = GenericCommandChannel<AdcsCommand>;
type AdcsEventChannel = GenericEventChannel<AdcsEvent>;
type EpsCommandChannel = GenericCommandChannel<EpsCommand>;
type EpsEventChannel = GenericEventChannel<EpsEvent>;

static ADCS_COMMANDS: AdcsCommandChannel = AdcsCommandChannel::new();
static ADCS_EVENTS: AdcsEventChannel = AdcsEventChannel::new();
static EPS_COMMANDS: EpsCommandChannel = EpsCommandChannel::new();
static EPS_EVENTS: EpsEventChannel = EpsEventChannel::new();

pub fn bundle() -> SubsystemsBundle {
    SubsystemsBundle {
        adcs: SubsystemLink {
            cmd_tx: ADCS_COMMANDS.sender(),
            event_rx: ADCS_EVENTS.subscriber().unwrap(),
        },
        eps: SubsystemLink {
            cmd_tx: EPS_COMMANDS.sender(),
            event_rx: EPS_EVENTS.subscriber().unwrap(),
        },
    }
}

pub fn system_handles() -> impl Sized {
    // TODO: передать эти handles в реальные AdcsSystem и EpsSystem.
    (
        (ADCS_COMMANDS.receiver(), ADCS_EVENTS.publisher().unwrap()),
        (EPS_COMMANDS.receiver(), EPS_EVENTS.publisher().unwrap()),
    )
}

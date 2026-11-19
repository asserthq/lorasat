use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use sat_core::{
    adcs::{AdcsCommand, AdcsEvent},
    eps::{EpsCommand, EpsEvent},
};
use sat_subsystems::{
    cdh::ControllerHandles,
    link::{ControllerHandle, WorkerHandle},
    payload::{PayloadCommand, PayloadEvent},
};

const COMMAND_QUEUE_DEPTH: usize = 4;
const EVENT_QUEUE_DEPTH: usize = 4;

type ChannelMutex = CriticalSectionRawMutex;

type GenericCommandChannel<C> = Channel<ChannelMutex, C, COMMAND_QUEUE_DEPTH>;
type GenericEventChannel<E> = Channel<ChannelMutex, E, EVENT_QUEUE_DEPTH>;

type AdcsCommandChannel = GenericCommandChannel<AdcsCommand>;
type AdcsEventChannel = GenericEventChannel<AdcsEvent>;
type EpsCommandChannel = GenericCommandChannel<EpsCommand>;
type EpsEventChannel = GenericEventChannel<EpsEvent>;
type ServiceCommandChannel = GenericCommandChannel<PayloadCommand>;
type ServiceEventChannel = GenericEventChannel<PayloadEvent>;

static ADCS_COMMANDS: AdcsCommandChannel = AdcsCommandChannel::new();
static ADCS_EVENTS: AdcsEventChannel = AdcsEventChannel::new();
static EPS_COMMANDS: EpsCommandChannel = EpsCommandChannel::new();
static EPS_EVENTS: EpsEventChannel = EpsEventChannel::new();
static SERVICE_COMMANDS: ServiceCommandChannel = ServiceCommandChannel::new();
static SERVICE_EVENTS: ServiceEventChannel = ServiceEventChannel::new();

pub struct WorkersHandles<'a> {
    pub adcs: WorkerHandle<'a, AdcsCommand, AdcsEvent>,
    pub eps: WorkerHandle<'a, EpsCommand, EpsEvent>,
    pub service: WorkerHandle<'a, PayloadCommand, PayloadEvent>,
}

pub fn split() -> (ControllerHandles<'static>, WorkersHandles<'static>) {
    let cdh_handles = ControllerHandles {
        adcs: ControllerHandle {
            cmd_tx: ADCS_COMMANDS.sender().into(),
            event_rx: ADCS_EVENTS.receiver().into(),
        },
        eps: ControllerHandle {
            cmd_tx: EPS_COMMANDS.sender().into(),
            event_rx: EPS_EVENTS.receiver().into(),
        },
        service: ControllerHandle {
            cmd_tx: SERVICE_COMMANDS.sender().into(),
            event_rx: SERVICE_EVENTS.receiver().into(),
        },
    };

    let systems_handles = WorkersHandles {
        adcs: WorkerHandle {
            cmd_rx: ADCS_COMMANDS.receiver().into(),
            event_tx: ADCS_EVENTS.sender().into(),
        },
        eps: WorkerHandle {
            cmd_rx: EPS_COMMANDS.receiver().into(),
            event_tx: EPS_EVENTS.sender().into(),
        },
        service: WorkerHandle {
            cmd_rx: SERVICE_COMMANDS.receiver().into(),
            event_tx: SERVICE_EVENTS.sender().into(),
        },
    };

    (cdh_handles, systems_handles)
}

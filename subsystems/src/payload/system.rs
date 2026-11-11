use core::{cell::RefCell, time::Duration};

use embassy_futures::{
    join::join3,
    select::{Either, select},
    yield_now,
};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::{DynamicReceiver, DynamicSender},
    signal::Signal,
};
use sat_core::{
    net::DatagramSocket,
    protocol::Beacon,
    storage::QueueStorage,
    time::{Clock, Delay},
};

use super::{
    BeaconConfig, ClientRecordRejection, MAX_CLIENT_PAYLOAD_SIZE, MAX_CLIENT_RECORD_SIZE,
    PayloadCommand, PayloadEvent, PayloadState, beacon::BEACON_BUFFER_SIZE, client_record_codec,
};

pub struct PayloadSystem<'a, BeaconSocket, ClientSocket, ClientStorage>
where
    BeaconSocket: DatagramSocket,
    ClientSocket: DatagramSocket,
    ClientStorage: QueueStorage,
{
    beacon_socket: BeaconSocket,
    client_socket: ClientSocket,
    storage: ClientStorage,
    commands: DynamicReceiver<'a, PayloadCommand>,
    events: DynamicSender<'a, PayloadEvent>,
    pending_event: Option<PayloadEvent>,
    state: PayloadState,
    next_beacon_at: Option<u64>,
}

impl<'a, BeaconSocket, ClientSocket, ClientStorage>
    PayloadSystem<'a, BeaconSocket, ClientSocket, ClientStorage>
where
    BeaconSocket: DatagramSocket,
    ClientSocket: DatagramSocket,
    ClientStorage: QueueStorage,
{
    pub const fn new(
        beacon_socket: BeaconSocket,
        client_socket: ClientSocket,
        storage: ClientStorage,
        events: DynamicSender<'a, PayloadEvent>,
        commands: DynamicReceiver<'a, PayloadCommand>,
        config: BeaconConfig,
    ) -> Self {
        Self {
            beacon_socket,
            client_socket,
            storage,
            commands,
            events,
            pending_event: None,
            state: PayloadState::new(config),
            next_beacon_at: None,
        }
    }

    pub const fn state(&self) -> &PayloadState {
        &self.state
    }

    pub async fn run(&mut self) -> ! {
        self.run_with_timer(&mut EmbassyTimer).await
    }

    async fn run_with_timer(&mut self, timer: &mut (impl Clock + Delay + Clone)) -> ! {
        let first_deadline = timer
            .now_micros()
            .saturating_add(interval_micros(self.state.beacon_config));
        self.next_beacon_at.get_or_insert(first_deadline);
        let runtime = RefCell::new(Runtime {
            state: &mut self.state,
            pending_event: &mut self.pending_event,
            next_beacon_at: &mut self.next_beacon_at,
        });
        let beacon_changes = Signal::new();
        let reception_changes = Signal::new();
        let mut beacon_timer = timer.clone();
        let mut receive_timer = timer.clone();
        join3(
            run_control(
                self.commands,
                self.events,
                &runtime,
                timer,
                &beacon_changes,
                &reception_changes,
            ),
            run_beacons(
                &mut self.beacon_socket,
                &mut beacon_timer,
                &beacon_changes,
                &runtime,
                self.events,
            ),
            run_client_receiver(
                &mut self.client_socket,
                &mut self.storage,
                &mut receive_timer,
                &reception_changes,
                &runtime,
                self.events,
            ),
        )
        .await;
        unreachable!("payload workers returned")
    }

    pub async fn process_next_command(&mut self) -> PayloadEvent {
        self.process_next_command_with_clock(&EmbassyTimer).await
    }

    async fn process_next_command_with_clock(&mut self, clock: &impl Clock) -> PayloadEvent {
        let command = self.commands.receive().await;
        let event = apply_command(command, &mut self.state);
        if matches!(command, PayloadCommand::ConfigureBeacon(_)) {
            let deadline = clock
                .now_micros()
                .saturating_add(interval_micros(self.state.beacon_config));
            self.next_beacon_at = Some(deadline);
        }
        event
    }
}

struct Runtime<'a> {
    state: &'a mut PayloadState,
    pending_event: &'a mut Option<PayloadEvent>,
    next_beacon_at: &'a mut Option<u64>,
}

fn interval_micros(config: BeaconConfig) -> u64 {
    u64::from(config.interval_seconds.get()) * 1_000_000
}

async fn run_control(
    commands: DynamicReceiver<'_, PayloadCommand>,
    events: DynamicSender<'_, PayloadEvent>,
    runtime: &RefCell<Runtime<'_>>,
    timer: &impl Clock,
    beacon_changes: &Signal<NoopRawMutex, ()>,
    reception_changes: &Signal<NoopRawMutex, ()>,
) -> ! {
    let mut budget = 16;
    loop {
        let pending = *runtime.borrow().pending_event;
        if let Some(event) = pending {
            events.send(event).await;
            *runtime.borrow_mut().pending_event = None;
        } else {
            let command = commands.receive().await;
            let reconfigured = matches!(command, PayloadCommand::ConfigureBeacon(_));
            let reception_changed;
            {
                let mut runtime = runtime.borrow_mut();
                let was_enabled = runtime.state.reception_enabled;
                let response = apply_command(command, runtime.state);
                reception_changed = was_enabled != runtime.state.reception_enabled;
                *runtime.pending_event = Some(response);
                if reconfigured {
                    let deadline = timer
                        .now_micros()
                        .saturating_add(interval_micros(runtime.state.beacon_config));
                    *runtime.next_beacon_at = Some(deadline);
                }
            }
            if reconfigured {
                beacon_changes.signal(());
            }
            if reception_changed {
                reception_changes.signal(());
            }
        }
        budget -= 1;
        if budget == 0 {
            budget = 16;
            yield_now().await;
        }
    }
}

async fn run_beacons(
    socket: &mut impl DatagramSocket,
    timer: &mut (impl Clock + Delay),
    changes: &Signal<NoopRawMutex, ()>,
    runtime: &RefCell<Runtime<'_>>,
    events: DynamicSender<'_, PayloadEvent>,
) -> ! {
    loop {
        changes.reset();
        let deadline = runtime
            .borrow()
            .next_beacon_at
            .expect("beacon schedule must be initialized");
        let remaining = deadline.saturating_sub(timer.now_micros());
        let wait = async {
            if remaining != 0 {
                timer.delay(Duration::from_micros(remaining)).await;
            }
        };
        if let Either::Second(()) = select(wait, changes.wait()).await {
            continue;
        }
        let config = {
            let mut runtime = runtime.borrow_mut();
            let now = timer.now_micros();
            if runtime.next_beacon_at.is_some_and(|next| next > now) {
                continue;
            }
            let config = runtime.state.beacon_config;
            *runtime.next_beacon_at = Some(now.saturating_add(interval_micros(config)));
            config
        };
        let beacon = Beacon {
            sat_addr: config.satellite_address,
            timestamp: timer.now_micros() / 1_000,
            interval_sec: config.interval_seconds.get(),
        };
        let mut buffer = [0; BEACON_BUFFER_SIZE];
        let bytes = beacon
            .encode(&mut buffer)
            .expect("Beacon must fit its postcard buffer");
        let destination = config.destination;
        let result = socket.send_to(destination, bytes).await;
        {
            let mut runtime = runtime.borrow_mut();
            let event = if result.is_ok() {
                runtime.state.transmitted_beacons =
                    runtime.state.transmitted_beacons.saturating_add(1);
                PayloadEvent::BeaconSent { destination }
            } else {
                runtime.state.beacon_send_errors =
                    runtime.state.beacon_send_errors.saturating_add(1);
                PayloadEvent::BeaconSendFailed { destination }
            };
            notify(&mut runtime, events, event);
            let now = timer.now_micros();
            if runtime.next_beacon_at.is_some_and(|next| next < now) {
                *runtime.next_beacon_at =
                    Some(now.saturating_add(interval_micros(runtime.state.beacon_config)));
            }
        }
        yield_now().await;
    }
}

fn notify(runtime: &mut Runtime<'_>, events: DynamicSender<'_, PayloadEvent>, event: PayloadEvent) {
    if runtime.pending_event.is_some() || events.try_send(event).is_err() {
        runtime.state.dropped_notifications = runtime.state.dropped_notifications.saturating_add(1);
    }
}

async fn run_client_receiver<Storage: QueueStorage>(
    socket: &mut impl DatagramSocket,
    storage: &mut Storage,
    timer: &mut (impl Clock + Delay),
    reception_changes: &Signal<NoopRawMutex, ()>,
    runtime: &RefCell<Runtime<'_>>,
    events: DynamicSender<'_, PayloadEvent>,
) -> ! {
    let mut payload = [0; MAX_CLIENT_PAYLOAD_SIZE];
    let mut record = [0; MAX_CLIENT_RECORD_SIZE];
    loop {
        reception_changes.reset();
        let reception_enabled = runtime.borrow().state.reception_enabled;
        if !reception_enabled {
            reception_changes.wait().await;
            continue;
        }
        let received = match select(reception_changes.wait(), socket.recv_from(&mut payload)).await
        {
            Either::First(()) => continue,
            Either::Second(result) => result,
        };
        let metadata = match received {
            Ok(metadata) => metadata,
            Err(_) => match socket
                .discard_oversized(payload.len())
                .filter(|metadata| metadata.length() > payload.len())
            {
                Some(metadata) => metadata,
                None => {
                    {
                        let mut runtime = runtime.borrow_mut();
                        runtime.state.receive_errors =
                            runtime.state.receive_errors.saturating_add(1);
                        notify(&mut runtime, events, PayloadEvent::ClientReceiveFailed);
                    }
                    timer.delay(Duration::from_millis(100)).await;
                    continue;
                }
            },
        };
        let received_at_ms = timer.now_micros() / 1_000;
        {
            let mut runtime = runtime.borrow_mut();
            runtime.state.received_datagrams = runtime.state.received_datagrams.saturating_add(1);
        }
        let source = metadata.source();
        let payload_length = metadata.length();
        if payload_length > payload.len() {
            {
                let mut runtime = runtime.borrow_mut();
                runtime.state.rejected_records = runtime.state.rejected_records.saturating_add(1);
                notify(
                    &mut runtime,
                    events,
                    PayloadEvent::ClientRecordRejected {
                        source,
                        payload_length,
                        reason: ClientRecordRejection::PayloadTooLong,
                    },
                );
            }
            yield_now().await;
            continue;
        }
        let length = client_record_codec::encode(
            source,
            received_at_ms,
            &payload[..payload_length],
            &mut record,
        )
        .expect("bounded client record must fit its buffer");
        let result = storage.append(&record[..length]).await;
        {
            let mut runtime = runtime.borrow_mut();
            let event = match result {
                Ok(()) => {
                    runtime.state.stored_records = runtime.state.stored_records.saturating_add(1);
                    runtime.state.last_client = Some(source);
                    PayloadEvent::ClientRecordStored {
                        source,
                        payload_length,
                    }
                }
                Err(error) => {
                    runtime.state.rejected_records =
                        runtime.state.rejected_records.saturating_add(1);
                    runtime.state.storage_errors = runtime.state.storage_errors.saturating_add(1);
                    PayloadEvent::ClientRecordRejected {
                        source,
                        payload_length,
                        reason: ClientRecordRejection::Storage(Storage::error_kind(&error)),
                    }
                }
            };
            notify(&mut runtime, events, event);
        }
        yield_now().await;
    }
}

fn apply_command(command: PayloadCommand, state: &mut PayloadState) -> PayloadEvent {
    match command {
        PayloadCommand::Ping => PayloadEvent::Pong,
        PayloadCommand::GetState => PayloadEvent::State(*state),
        PayloadCommand::SetReceptionEnabled(enabled) => {
            state.reception_enabled = enabled;
            PayloadEvent::CommandCompleted(command)
        }
        PayloadCommand::ConfigureBeacon(config) => {
            state.beacon_config = config;
            PayloadEvent::CommandCompleted(command)
        }
    }
}

#[derive(Clone, Copy)]
struct EmbassyTimer;

impl Clock for EmbassyTimer {
    fn now_micros(&self) -> u64 {
        embassy_time::Instant::now().as_micros()
    }
}

impl Delay for EmbassyTimer {
    async fn delay(&mut self, duration: Duration) {
        embassy_time::Timer::after(embassy_time::Duration::from_micros(
            duration.as_micros() as u64
        ))
        .await;
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

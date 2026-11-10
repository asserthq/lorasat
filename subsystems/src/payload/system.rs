use embassy_sync::channel::{DynamicReceiver, DynamicSender};
use sat_core::{net::DatagramSocket, storage::QueueStorage};

use super::{PayloadCommand, PayloadEvent};

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
    ) -> Self {
        Self {
            beacon_socket,
            client_socket,
            storage,
            commands,
            events,
            pending_event: None,
        }
    }

    pub async fn run(&mut self) -> ! {
        loop {
            let event = match self.pending_event {
                Some(event) => event,
                None => {
                    let event = self.process_next_command().await;
                    self.pending_event = Some(event);
                    event
                }
            };
            self.events.send(event).await;
            self.pending_event = None;
        }
    }

    pub async fn process_next_command(&mut self) -> PayloadEvent {
        match self.commands.receive().await {
            PayloadCommand::Ping => PayloadEvent::Pong,
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

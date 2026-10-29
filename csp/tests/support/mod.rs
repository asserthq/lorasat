#![allow(dead_code)]

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use futures::{
    SinkExt, StreamExt,
    channel::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender},
};
use sat_csp::CspInterface;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FakeLinkError {
    Disconnected,
    FrameTooLarge,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum NextAction {
    #[default]
    Deliver,
    Drop,
    Duplicate,
    Hold,
}

#[derive(Debug, Default)]
struct DirectionState {
    next_action: NextAction,
    held: VecDeque<Vec<u8>>,
    transmitted_frames: Vec<Vec<u8>>,
    transmissions: usize,
}

pub struct FakeLink {
    outbound: Sender<Vec<u8>>,
    inbound: Receiver<Vec<u8>>,
    state: Arc<Mutex<DirectionState>>,
    events: UnboundedSender<()>,
}

pub struct FakeLinkControl {
    outbound: Sender<Vec<u8>>,
    state: Arc<Mutex<DirectionState>>,
    events: UnboundedReceiver<()>,
}

pub fn fake_link_pair(
    capacity: usize,
) -> ((FakeLink, FakeLinkControl), (FakeLink, FakeLinkControl)) {
    let (a_to_b_tx, a_to_b_rx) = mpsc::channel(capacity);
    let (b_to_a_tx, b_to_a_rx) = mpsc::channel(capacity);

    let (a_events_tx, a_events_rx) = mpsc::unbounded();
    let (b_events_tx, b_events_rx) = mpsc::unbounded();
    let a_state = Arc::new(Mutex::new(DirectionState::default()));
    let b_state = Arc::new(Mutex::new(DirectionState::default()));

    (
        (
            FakeLink {
                outbound: a_to_b_tx.clone(),
                inbound: b_to_a_rx,
                state: Arc::clone(&a_state),
                events: a_events_tx,
            },
            FakeLinkControl {
                outbound: a_to_b_tx,
                state: a_state,
                events: a_events_rx,
            },
        ),
        (
            FakeLink {
                outbound: b_to_a_tx.clone(),
                inbound: a_to_b_rx,
                state: Arc::clone(&b_state),
                events: b_events_tx,
            },
            FakeLinkControl {
                outbound: b_to_a_tx,
                state: b_state,
                events: b_events_rx,
            },
        ),
    )
}

impl FakeLinkControl {
    pub fn drop_next(&self) {
        self.state.lock().unwrap().next_action = NextAction::Drop;
    }

    pub fn duplicate_next(&self) {
        self.state.lock().unwrap().next_action = NextAction::Duplicate;
    }

    pub fn hold_next(&self) {
        self.state.lock().unwrap().next_action = NextAction::Hold;
    }

    pub async fn release_held(&mut self) -> Result<(), FakeLinkError> {
        let frame = self.state.lock().unwrap().held.pop_front();
        if let Some(frame) = frame {
            self.outbound
                .send(frame)
                .await
                .map_err(|_| FakeLinkError::Disconnected)?;
        }
        Ok(())
    }

    pub async fn inject(&mut self, frame: &[u8]) -> Result<(), FakeLinkError> {
        self.outbound
            .send(frame.to_vec())
            .await
            .map_err(|_| FakeLinkError::Disconnected)
    }

    pub async fn wait_for_transmission(&mut self) {
        let _ = self.events.next().await;
    }

    pub fn transmission_count(&self) -> usize {
        self.state.lock().unwrap().transmissions
    }

    pub fn transmitted_frames(&self) -> Vec<Vec<u8>> {
        self.state.lock().unwrap().transmitted_frames.clone()
    }
}

impl CspInterface for FakeLink {
    type Error = FakeLinkError;

    async fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        let action = {
            let mut state = self.state.lock().unwrap();
            state.transmissions += 1;
            state.transmitted_frames.push(frame.to_vec());
            let action = state.next_action;
            state.next_action = NextAction::Deliver;
            if action == NextAction::Hold {
                state.held.push_back(frame.to_vec());
            }
            action
        };
        let _ = self.events.unbounded_send(());

        match action {
            NextAction::Deliver => self
                .outbound
                .send(frame.to_vec())
                .await
                .map_err(|_| FakeLinkError::Disconnected),
            NextAction::Drop | NextAction::Hold => Ok(()),
            NextAction::Duplicate => {
                self.outbound
                    .send(frame.to_vec())
                    .await
                    .map_err(|_| FakeLinkError::Disconnected)?;
                self.outbound
                    .send(frame.to_vec())
                    .await
                    .map_err(|_| FakeLinkError::Disconnected)
            }
        }
    }

    async fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error> {
        let received = self
            .inbound
            .next()
            .await
            .ok_or(FakeLinkError::Disconnected)?;
        if received.len() > frame.len() {
            return Err(FakeLinkError::FrameTooLarge);
        }
        frame[..received.len()].copy_from_slice(&received);
        Ok(received.len())
    }
}

use sat_core::{net::Endpoint, storage::QueueErrorKind};

use super::{PayloadCommand, PayloadState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientRecordRejection {
    PayloadTooLong,
    Storage(QueueErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadEvent {
    Pong,
    CommandCompleted(PayloadCommand),
    State(PayloadState),
    BeaconSent {
        destination: Endpoint,
    },
    BeaconSendFailed {
        destination: Endpoint,
    },
    ClientRecordStored {
        source: Endpoint,
        payload_length: usize,
    },
    ClientRecordRejected {
        source: Endpoint,
        payload_length: usize,
        reason: ClientRecordRejection,
    },
    ClientReceiveFailed,
}

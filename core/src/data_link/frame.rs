use super::data::Data;
use crate::message::Beacon;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    BeaconFrame(Beacon),
    DataFrame(Data),
}

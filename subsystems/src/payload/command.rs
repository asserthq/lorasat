use super::BeaconConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadCommand {
    Ping,
    ConfigureBeacon(BeaconConfig),
    SetReceptionEnabled(bool),
    GetState,
}

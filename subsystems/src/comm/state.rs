use sat_core::comm::Endpoint;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CommState {
    pub transmitted_beacons: u32,
    pub received_datagrams: u32,
    pub dropped_datagrams: u32,
    pub storage_errors: u32,
    pub receive_errors: u32,
    pub beacon_encode_errors: u32,
    pub beacon_send_errors: u32,
    pub last_client: Option<Endpoint>,
}

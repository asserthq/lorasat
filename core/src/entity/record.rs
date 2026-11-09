use crate::net::Endpoint;

pub struct ClientRecord<'a> {
    pub source: Endpoint,
    pub received_at_ms: u64,
    pub payload: &'a [u8],
}

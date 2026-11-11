use sat_core::{
    domain::record::ClientRecord,
    net::{Address, Endpoint},
};

const VERSION: u8 = 1;
pub const HEADER_SIZE: usize = 16;
pub const MAX_CLIENT_PAYLOAD_SIZE: usize = 256;
pub const MAX_CLIENT_RECORD_SIZE: usize = HEADER_SIZE + MAX_CLIENT_PAYLOAD_SIZE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    BufferTooSmall { required: usize, available: usize },
    PayloadTooLong(usize),
    InvalidSourceAddress(u32),
    Truncated,
    UnsupportedVersion(u8),
    TrailingBytes,
}

pub fn encode(
    source: Endpoint,
    received_at_ms: u64,
    payload: &[u8],
    buffer: &mut [u8],
) -> Result<usize, Error> {
    let payload_length =
        u16::try_from(payload.len()).map_err(|_| Error::PayloadTooLong(payload.len()))?;
    let required = HEADER_SIZE + payload.len();
    if buffer.len() < required {
        return Err(Error::BufferTooSmall {
            required,
            available: buffer.len(),
        });
    }

    buffer[0] = VERSION;
    buffer[1..5].copy_from_slice(&u32::from(source.addr().get()).to_be_bytes());
    buffer[5] = source.port();
    buffer[6..14].copy_from_slice(&received_at_ms.to_be_bytes());
    buffer[14..16].copy_from_slice(&payload_length.to_be_bytes());
    buffer[HEADER_SIZE..required].copy_from_slice(payload);
    Ok(required)
}

pub fn decode(buffer: &[u8]) -> Result<ClientRecord<'_>, Error> {
    if buffer.len() < HEADER_SIZE {
        return Err(Error::Truncated);
    }
    if buffer[0] != VERSION {
        return Err(Error::UnsupportedVersion(buffer[0]));
    }

    let payload_length = usize::from(u16::from_be_bytes([buffer[14], buffer[15]]));
    let required = HEADER_SIZE + payload_length;
    if buffer.len() < required {
        return Err(Error::Truncated);
    }
    if buffer.len() != required {
        return Err(Error::TrailingBytes);
    }

    let raw_address = u32::from_be_bytes([buffer[1], buffer[2], buffer[3], buffer[4]]);
    let address = u16::try_from(raw_address)
        .ok()
        .and_then(Address::new)
        .ok_or(Error::InvalidSourceAddress(raw_address))?;
    let received_at_ms = u64::from_be_bytes([
        buffer[6], buffer[7], buffer[8], buffer[9], buffer[10], buffer[11], buffer[12], buffer[13],
    ]);

    Ok(ClientRecord {
        source: Endpoint::new(address, buffer[5]),
        received_at_ms,
        payload: &buffer[HEADER_SIZE..required],
    })
}

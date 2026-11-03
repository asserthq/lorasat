use sat_core::comm::{ClientRecord, Endpoint};

const VERSION: u8 = 1;
const HEADER_SIZE: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    BufferTooSmall { required: usize, available: usize },
    PayloadTooLong(usize),
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
    buffer[1..5].copy_from_slice(&source.addr().0.to_be_bytes());
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

    let address = u32::from_be_bytes([buffer[1], buffer[2], buffer[3], buffer[4]]);
    let received_at_ms = u64::from_be_bytes([
        buffer[6], buffer[7], buffer[8], buffer[9], buffer[10], buffer[11], buffer[12], buffer[13],
    ]);

    Ok(ClientRecord {
        source: Endpoint::new(sat_core::comm::Address(address), buffer[5]),
        received_at_ms,
        payload: &buffer[HEADER_SIZE..required],
    })
}

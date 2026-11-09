use sat_core::comm::ClientRecord;

pub const DUMP_COMMAND: &[u8] = b"dump";
const DUMP_VERSION: u8 = 1;
const RESPONSE_HEADER_SIZE: usize = 3;
const RECORD_HEADER_SIZE: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    BufferTooSmall { required: usize, available: usize },
    InvalidAddress(u32),
    PayloadTooLong(usize),
}

pub fn encode_empty_dump(buffer: &mut [u8]) -> Result<usize, Error> {
    if buffer.len() < RESPONSE_HEADER_SIZE {
        return Err(Error::BufferTooSmall {
            required: RESPONSE_HEADER_SIZE,
            available: buffer.len(),
        });
    }

    buffer[..RESPONSE_HEADER_SIZE].copy_from_slice(&[DUMP_VERSION, 0, 0]);
    Ok(RESPONSE_HEADER_SIZE)
}

pub fn encode_dump_record(record: &ClientRecord<'_>, buffer: &mut [u8]) -> Result<usize, Error> {
    let source = u16::try_from(record.source.addr().0)
        .map_err(|_| Error::InvalidAddress(record.source.addr().0))?;
    let payload_length = u8::try_from(record.payload.len())
        .map_err(|_| Error::PayloadTooLong(record.payload.len()))?;
    let required = RESPONSE_HEADER_SIZE + RECORD_HEADER_SIZE + record.payload.len();
    if buffer.len() < required {
        return Err(Error::BufferTooSmall {
            required,
            available: buffer.len(),
        });
    }

    buffer[..RESPONSE_HEADER_SIZE].copy_from_slice(&[DUMP_VERSION, 1, 1]);
    buffer[3..5].copy_from_slice(&source.to_be_bytes());
    buffer[5] = record.source.port();
    buffer[6] = payload_length;
    buffer[7..required].copy_from_slice(record.payload);
    Ok(required)
}

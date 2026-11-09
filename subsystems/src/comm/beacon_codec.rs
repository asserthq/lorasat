use sat_core::entity::Beacon;

const VERSION: u8 = 1;
pub const ENCODED_SIZE: usize = 13;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    BufferTooSmall { required: usize, available: usize },
    InvalidSatelliteAddress(u32),
}

pub fn encode(beacon: &Beacon, buffer: &mut [u8]) -> Result<usize, Error> {
    if buffer.len() < ENCODED_SIZE {
        return Err(Error::BufferTooSmall {
            required: ENCODED_SIZE,
            available: buffer.len(),
        });
    }

    let satellite_address = u16::try_from(beacon.sat_addr.0)
        .map_err(|_| Error::InvalidSatelliteAddress(beacon.sat_addr.0))?;

    buffer[0] = VERSION;
    buffer[1..3].copy_from_slice(&satellite_address.to_be_bytes());
    buffer[3..11].copy_from_slice(&beacon.timestamp.to_be_bytes());
    buffer[11..13].copy_from_slice(&beacon.interval_sec.to_be_bytes());
    Ok(ENCODED_SIZE)
}

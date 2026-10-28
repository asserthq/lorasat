use crate::{Error, Flags, HEADER_SIZE, Header, Packet, Priority};

const PRIORITY_OFFSET: u32 = 46;
const DESTINATION_OFFSET: u32 = 32;
const SOURCE_OFFSET: u32 = 18;
const DESTINATION_PORT_OFFSET: u32 = 12;
const SOURCE_PORT_OFFSET: u32 = 6;

const PRIORITY_MASK: u64 = 0x03;
const NODE_MASK: u64 = 0x3FFF;
const PORT_MASK: u64 = 0x3F;
const FLAGS_MASK: u64 = 0x3F;

pub(crate) fn encode_header(header: Header, output: &mut [u8]) -> Result<usize, Error> {
    if output.len() < HEADER_SIZE {
        return Err(Error::OutputTooShort {
            required: HEADER_SIZE,
            actual: output.len(),
        });
    }

    let raw = (u64::from(header.priority().bits()) << PRIORITY_OFFSET)
        | (u64::from(header.destination()) << DESTINATION_OFFSET)
        | (u64::from(header.source()) << SOURCE_OFFSET)
        | (u64::from(header.destination_port()) << DESTINATION_PORT_OFFSET)
        | (u64::from(header.source_port()) << SOURCE_PORT_OFFSET)
        | u64::from(header.flags().bits());
    let bytes = raw.to_be_bytes();
    output[..HEADER_SIZE].copy_from_slice(&bytes[2..]);

    Ok(HEADER_SIZE)
}

pub(crate) fn encode_packet<const MTU: usize>(
    packet: &Packet<MTU>,
    output: &mut [u8],
) -> Result<usize, Error> {
    let required = packet.encoded_len();
    if output.len() < required {
        return Err(Error::OutputTooShort {
            required,
            actual: output.len(),
        });
    }

    encode_header(packet.header(), output)?;
    output[HEADER_SIZE..required].copy_from_slice(packet.payload());
    Ok(required)
}

pub(crate) fn decode_packet<const MTU: usize>(input: &[u8]) -> Result<Packet<MTU>, Error> {
    if MTU < HEADER_SIZE {
        return Err(Error::MtuTooSmall {
            minimum: HEADER_SIZE,
            actual: MTU,
        });
    }
    if input.len() < HEADER_SIZE {
        return Err(Error::InputTooShort {
            required: HEADER_SIZE,
            actual: input.len(),
        });
    }
    if input.len() > MTU {
        return Err(Error::PacketTooLarge {
            maximum: MTU,
            actual: input.len(),
        });
    }

    let header = decode_header(input)?;
    Packet::new(header, &input[HEADER_SIZE..])
}

pub(crate) fn decode_header(input: &[u8]) -> Result<Header, Error> {
    if input.len() < HEADER_SIZE {
        return Err(Error::InputTooShort {
            required: HEADER_SIZE,
            actual: input.len(),
        });
    }

    let mut bytes = [0; 8];
    bytes[2..].copy_from_slice(&input[..HEADER_SIZE]);
    let raw = u64::from_be_bytes(bytes);

    let priority = Priority::from_bits(((raw >> PRIORITY_OFFSET) & PRIORITY_MASK) as u8);
    let destination = ((raw >> DESTINATION_OFFSET) & NODE_MASK) as u16;
    let source = ((raw >> SOURCE_OFFSET) & NODE_MASK) as u16;
    let destination_port = ((raw >> DESTINATION_PORT_OFFSET) & PORT_MASK) as u8;
    let source_port = ((raw >> SOURCE_PORT_OFFSET) & PORT_MASK) as u8;
    let flags = Flags::from_bits((raw & FLAGS_MASK) as u8)?;

    Header::new(
        priority,
        source,
        destination,
        source_port,
        destination_port,
        flags,
    )
}

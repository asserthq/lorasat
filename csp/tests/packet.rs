use sat_csp::{Error, Flags, HEADER_SIZE, Header, Packet, Priority};

fn header() -> Header {
    Header::new(Priority::Normal, 0x1234, 0x2345, 0x15, 0x2A, Flags::NONE).unwrap()
}

#[test]
fn empty_and_maximum_payload_round_trip() {
    let empty = Packet::<255>::new(header(), &[]).unwrap();
    let mut empty_bytes = [0; 255];
    let empty_len = empty.encode(&mut empty_bytes).unwrap();
    assert_eq!(empty_len, HEADER_SIZE);
    assert_eq!(
        Packet::<255>::decode(&empty_bytes[..empty_len]).unwrap(),
        empty
    );

    let payload = [0xA5; 255 - HEADER_SIZE];
    let packet = Packet::<255>::new(header(), &payload).unwrap();
    let mut bytes = [0; 255];
    let len = packet.encode(&mut bytes).unwrap();
    assert_eq!(len, 255);
    assert_eq!(Packet::<255>::decode(&bytes).unwrap(), packet);
}

#[test]
fn packet_above_mtu_is_rejected_without_truncation() {
    let payload = [0xA5; 250];

    assert_eq!(
        Packet::<255>::new(header(), &payload),
        Err(Error::PacketTooLarge {
            maximum: 255,
            actual: 256,
        })
    );

    let input = [0; 256];
    assert_eq!(
        Packet::<255>::decode(&input),
        Err(Error::PacketTooLarge {
            maximum: 255,
            actual: 256,
        })
    );
}

#[test]
fn short_packet_buffers_return_errors() {
    assert_eq!(
        Packet::<255>::decode(&[0; HEADER_SIZE - 1]),
        Err(Error::InputTooShort {
            required: HEADER_SIZE,
            actual: HEADER_SIZE - 1,
        })
    );

    let packet = Packet::<255>::new(header(), b"hello").unwrap();
    let mut output = [0; HEADER_SIZE + 5 - 1];
    assert_eq!(
        packet.encode(&mut output),
        Err(Error::OutputTooShort {
            required: HEADER_SIZE + 5,
            actual: HEADER_SIZE + 5 - 1,
        })
    );
}

#[test]
fn mtu_smaller_than_header_is_rejected() {
    assert_eq!(
        Packet::<5>::new(header(), &[]),
        Err(Error::MtuTooSmall {
            minimum: HEADER_SIZE,
            actual: 5,
        })
    );
    assert_eq!(
        Packet::<5>::decode(&[0; HEADER_SIZE]),
        Err(Error::MtuTooSmall {
            minimum: HEADER_SIZE,
            actual: 5,
        })
    );
}

#[test]
fn optional_protocol_flags_are_rejected_until_implemented() {
    let flagged_header = Header::new(Priority::Normal, 1, 2, 3, 4, Flags::CRC32).unwrap();

    assert_eq!(
        Packet::<255>::new(flagged_header, b"payload"),
        Err(Error::UnsupportedFlags(Flags::CRC32.bits()))
    );

    let mut bytes = [0; HEADER_SIZE];
    flagged_header.encode(&mut bytes).unwrap();
    assert_eq!(
        Packet::<255>::decode(&bytes),
        Err(Error::UnsupportedFlags(Flags::CRC32.bits()))
    );
}

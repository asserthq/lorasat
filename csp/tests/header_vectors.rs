use sat_csp::{Flags, HEADER_SIZE, Header, Priority};

#[test]
fn libcsp_v21_header_vectors_match_byte_for_byte() {
    let cases = [
        (
            Header::new(Priority::Critical, 0, 0, 0, 0, Flags::NONE).unwrap(),
            [0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        ),
        (
            Header::new(Priority::Normal, 0x1234, 0x2345, 0x15, 0x2A, Flags::NONE).unwrap(),
            [0xA3, 0x45, 0x48, 0xD2, 0xA5, 0x40],
        ),
        (
            Header::new(Priority::High, 2, 1, 4, 3, Flags::NONE).unwrap(),
            [0x40, 0x01, 0x00, 0x08, 0x31, 0x00],
        ),
        (
            Header::new(
                Priority::Low,
                0x3FFF,
                0x3FFF,
                0x3F,
                0x3F,
                Flags::from_bits(0x3F).unwrap(),
            )
            .unwrap(),
            [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        ),
    ];

    for (header, expected) in cases {
        let mut encoded = [0; HEADER_SIZE];
        assert_eq!(header.encode(&mut encoded).unwrap(), HEADER_SIZE);
        assert_eq!(encoded, expected);
        assert_eq!(Header::decode(&encoded).unwrap(), header);
    }
}

#[test]
fn source_destination_and_ports_keep_their_direction() {
    let encoded = [0x40, 0x01, 0x00, 0x08, 0x31, 0x00];
    let header = Header::decode(&encoded).unwrap();

    assert_eq!(header.source(), 2);
    assert_eq!(header.destination(), 1);
    assert_eq!(header.source_port(), 4);
    assert_eq!(header.destination_port(), 3);
}

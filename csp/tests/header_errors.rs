use sat_csp::{Error, Flags, HEADER_SIZE, Header, MAX_NODE_ID, MAX_PORT, Priority};

#[test]
fn rejects_fields_outside_csp_v2_ranges() {
    assert_eq!(
        Header::new(Priority::Normal, MAX_NODE_ID + 1, 0, 0, 0, Flags::NONE,),
        Err(Error::InvalidSource(MAX_NODE_ID + 1))
    );
    assert_eq!(
        Header::new(Priority::Normal, 0, MAX_NODE_ID + 1, 0, 0, Flags::NONE,),
        Err(Error::InvalidDestination(MAX_NODE_ID + 1))
    );
    assert_eq!(
        Header::new(Priority::Normal, 0, 0, MAX_PORT + 1, 0, Flags::NONE,),
        Err(Error::InvalidSourcePort(MAX_PORT + 1))
    );
    assert_eq!(
        Header::new(Priority::Normal, 0, 0, 0, MAX_PORT + 1, Flags::NONE,),
        Err(Error::InvalidDestinationPort(MAX_PORT + 1))
    );
    assert_eq!(Flags::from_bits(0x40), Err(Error::InvalidFlags(0x40)));
}

#[test]
fn short_header_buffers_return_errors() {
    let header = Header::new(Priority::Normal, 1, 2, 3, 4, Flags::NONE).unwrap();
    let mut output = [0; HEADER_SIZE - 1];

    assert_eq!(
        header.encode(&mut output),
        Err(Error::OutputTooShort {
            required: HEADER_SIZE,
            actual: HEADER_SIZE - 1,
        })
    );
    assert_eq!(
        Header::decode(&output),
        Err(Error::InputTooShort {
            required: HEADER_SIZE,
            actual: HEADER_SIZE - 1,
        })
    );
}

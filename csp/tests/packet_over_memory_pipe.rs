use sat_csp::{Flags, Header, Packet, Priority};

const MTU: usize = 255;
const LIBCSP_PACKET: [u8; 11] = [
    0xA3, 0x45, 0x48, 0xD2, 0xA5, 0x40, 0x68, 0x65, 0x6C, 0x6C, 0x6F,
];

struct MemoryPipe {
    queued: Option<([u8; MTU], usize)>,
}

impl MemoryPipe {
    fn new() -> Self {
        Self { queued: None }
    }

    fn send(&mut self, frame: &[u8]) {
        assert!(self.queued.is_none());
        let mut bytes = [0; MTU];
        bytes[..frame.len()].copy_from_slice(frame);
        self.queued = Some((bytes, frame.len()));
    }

    fn receive(&mut self) -> ([u8; MTU], usize) {
        self.queued.take().expect("one frame must be queued")
    }
}

#[test]
fn packet_over_memory_pipe() {
    let header = Header::new(Priority::Normal, 0x1234, 0x2345, 0x15, 0x2A, Flags::NONE).unwrap();
    let outgoing = Packet::<MTU>::new(header, b"hello").unwrap();

    let mut encoded = [0; MTU];
    let encoded_len = outgoing.encode(&mut encoded).unwrap();
    assert_eq!(&encoded[..encoded_len], &LIBCSP_PACKET);

    let mut pipe = MemoryPipe::new();
    pipe.send(&encoded[..encoded_len]);
    let (received_bytes, received_len) = pipe.receive();

    let incoming = Packet::<MTU>::decode(&received_bytes[..received_len]).unwrap();
    assert_eq!(incoming.header().priority(), Priority::Normal);
    assert_eq!(incoming.header().source(), 0x1234);
    assert_eq!(incoming.header().destination(), 0x2345);
    assert_eq!(incoming.header().source_port(), 0x15);
    assert_eq!(incoming.header().destination_port(), 0x2A);
    assert_eq!(incoming.header().flags(), Flags::NONE);
    assert_eq!(incoming.payload(), b"hello");
}

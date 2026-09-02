use core::fmt::Debug;
use heapless::Vec;

use crate::layer::link::{Frame, FrameHeader, FrameKind, LinkLayer};
use crate::layer::transport::{MAX_TRANSPORT_CHUNK_PAYLOAD, Packet, TransportLayer};

pub struct SimpleTransport<L: LinkLayer + Debug> {
    addr: u32,
    link: L,
    buf: [u8; MAX_TRANSPORT_CHUNK_PAYLOAD],
}

impl<L: LinkLayer + Debug> SimpleTransport<L> {
    pub fn new(addr: u32, link: L) -> Self {
        Self {
            addr,
            link,
            buf: [0u8; MAX_TRANSPORT_CHUNK_PAYLOAD],
        }
    }
}

impl<L: LinkLayer + Debug> TransportLayer for SimpleTransport<L> {
    type Error = SimpleError<L>;

    async fn send_message(&mut self, msg: Packet) -> Result<(), Self::Error> {
        let ser = postcard::to_slice(&msg, &mut self.buf).unwrap();
        let datalink_payload = Vec::from_slice(ser).unwrap();

        let frame = Frame {
            header: FrameHeader {
                kind: FrameKind::Transport,
                src_addr: self.addr,
                dest_addr: msg.header.dest_addr,
                flags: 0u8,
            },
            payload: datalink_payload,
        };

        self.link.send_frame(frame).await.unwrap();
        Ok(())
    }

    async fn recv_message<'a>(&'a mut self, buf: &'a mut [u8]) -> Result<Packet, Self::Error> {
        let frame = self.link.recv_frame(buf).await.unwrap();
        let datalink_payload = frame.payload;
        let msg = postcard::from_bytes(&datalink_payload).unwrap();
        Ok(msg)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SimpleError<L: LinkLayer + Debug> {
    Error,
    DataLink(L::Error),
}

use core::fmt::Debug;
use heapless::Vec;

use sat_core::layer::link::{Frame, FrameHeader, FrameKind, LinkLayer};
use sat_core::layer::transport::{MAX_TRANSPORT_CHUNK_PAYLOAD, Packet, TransportLayer};

pub struct SimpleTransport<L: LinkLayer> {
    addr: u32,
    link: L,
    buf: [u8; MAX_TRANSPORT_CHUNK_PAYLOAD],
}

impl<L: LinkLayer> SimpleTransport<L> {
    pub fn new(addr: u32, link: L) -> Self {
        Self {
            addr,
            link,
            buf: [0u8; MAX_TRANSPORT_CHUNK_PAYLOAD],
        }
    }
}

impl<L: LinkLayer> TransportLayer for SimpleTransport<L> {
    type Error = SimpleError<L::Error>;

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
        let (msg, _src) = self.recv_message_inner(buf).await?;
        Ok(msg)
    }
}

impl<L: LinkLayer> SimpleTransport<L> {
    pub async fn recv_message_with_src<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> Result<(Packet, u32), SimpleError<L::Error>> {
        self.recv_message_inner(buf).await
    }

    async fn recv_message_inner<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> Result<(Packet, u32), SimpleError<L::Error>> {
        let frame = self.link.recv_frame(buf).await.unwrap();
        let src_addr = frame.header.src_addr;
        let datalink_payload = frame.payload;
        let msg = postcard::from_bytes(&datalink_payload).unwrap();
        Ok((msg, src_addr))
    }
}

#[derive(defmt::Format, Debug, PartialEq, Eq)]
pub enum SimpleError<LE: Debug> {
    Error,
    DataLink(LE),
}

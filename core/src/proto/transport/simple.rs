use core::fmt::Debug;
use heapless::Vec;

use crate::layer::data_link::{DataLinkFrame, DataLinkLayer, FrameHeader, FrameKind};
use crate::layer::transport::{MAX_TRANSPORT_CHUNK_PAYLOAD, TransportLayer, TransportMessage};

pub struct SimpleTransport<L: DataLinkLayer + Debug> {
    addr: u32,
    link: L,
    buf: [u8; MAX_TRANSPORT_CHUNK_PAYLOAD],
}

impl<L: DataLinkLayer + Debug> SimpleTransport<L> {
    pub fn new(addr: u32, link: L) -> Self {
        Self {
            addr,
            link,
            buf: [0u8; MAX_TRANSPORT_CHUNK_PAYLOAD],
        }
    }
}

impl<L: DataLinkLayer + Debug> TransportLayer for SimpleTransport<L> {
    type Error = SimpleError<L>;

    async fn try_send_message(&mut self, msg: TransportMessage) -> Result<(), Self::Error> {
        let ser = postcard::to_slice(&msg, &mut self.buf).unwrap();
        let datalink_payload = Vec::from_slice(ser).unwrap();

        let frame = DataLinkFrame {
            header: FrameHeader {
                kind: FrameKind::Transport,
                src_addr: self.addr,
                dest_addr: msg.header.dest_addr,
                flags: 0u8,
            },
            payload: datalink_payload,
        };

        self.link.try_send_frame(frame).await.unwrap();
        Ok(())
    }

    async fn try_recv_message<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> Result<TransportMessage, Self::Error> {
        let frame = self.link.try_recv_frame(buf).await.unwrap();
        let datalink_payload = frame.payload;
        let msg = postcard::from_bytes(&datalink_payload).unwrap();
        Ok(msg)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SimpleError<L: DataLinkLayer + Debug> {
    Error,
    DataLink(L::Error),
}

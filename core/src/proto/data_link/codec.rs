use crate::layer::data_link::{DataLinkFrame, DataLinkLayer};
use crate::layer::physical::PhysicalLayer;

use core::fmt::Debug;

use super::codec_error::CodecError;

/// Postcard-based DataLinkLayer implementation.
///
/// Serializes `DataLinkFrame` to bytes for `PhysicalLayer`.
/// Input: `DataLinkFrame` (from Transport or Application).
/// Output: bytes to Physical.
#[derive(Debug)]
pub struct DataLinkCodec<P: PhysicalLayer + Debug> {
    phy: P,
}

impl<P: PhysicalLayer + Debug> DataLinkCodec<P> {
    pub fn new(phy: P) -> Self {
        Self { phy }
    }

    fn encode<'a>(frame: &DataLinkFrame, buf: &'a mut [u8]) -> Result<&'a mut [u8], CodecError<P>> {
        postcard::to_slice(frame, buf).map_err(CodecError::from)
    }

    fn decode(raw: &[u8]) -> Result<DataLinkFrame, CodecError<P>> {
        postcard::from_bytes(raw).map_err(CodecError::from)
    }
}

impl<P: PhysicalLayer + Debug> DataLinkLayer for DataLinkCodec<P> {
    type Error = CodecError<P>;

    async fn send_frame(&mut self, frame: DataLinkFrame) -> Result<(), Self::Error> {
        let mut buf = [0u8; 256];
        let payload = Self::encode(&frame, &mut buf)?;
        self.phy
            .send_bytes(payload)
            .await
            .map_err(CodecError::Physical)
    }

    async fn recv_frame(&mut self, buf: &mut [u8]) -> Result<DataLinkFrame, Self::Error> {
        let payload = self
            .phy
            .recv_bytes(buf)
            .await
            .map_err(CodecError::Physical)?;
        Self::decode(payload)
    }
}

// ── tests ──

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::layer::data_link::FrameKind;
//     use crate::layer::physical::PhysicalLayer;
//     use core::fmt;
//     use core::future::Future;
//     use core::pin::Pin;
//     use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
//     use heapless::Vec;

//     // ── mock PhysicalLayer ──

//     #[derive(Debug)]
//     struct MockError;

//     impl fmt::Display for MockError {
//         fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//             f.write_str("mock")
//         }
//     }

//     /// PhysicalLayer mock: stores sent bytes, replays pre-loaded bytes.
//     struct MockPhy {
//         sent: Vec<u8, 256>,
//         recv: Vec<u8, 256>,
//     }

//     impl MockPhy {
//         fn new() -> Self {
//             Self {
//                 sent: Vec::new(),
//                 recv: Vec::new(),
//             }
//         }

//         fn set_recv(&mut self, data: &[u8]) {
//             self.recv = Vec::from_slice(data).unwrap();
//         }
//     }

//     impl fmt::Debug for MockPhy {
//         fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//             f.debug_struct("MockPhy").finish()
//         }
//     }

//     impl PhysicalLayer for MockPhy {
//         type Error = MockError;

//         async fn try_send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error> {
//             self.sent = Vec::from_slice(payload).unwrap();
//             Ok(())
//         }

//         async fn try_recv_bytes<'a>(
//             &mut self,
//             buf: &'a mut [u8],
//         ) -> Result<&'a mut [u8], Self::Error> {
//             let len = self.recv.len();
//             buf[..len].copy_from_slice(&self.recv);
//             Ok(&mut buf[..len])
//         }
//     }

//     // ── helpers ──

//     fn make_frame(kind: FrameKind) -> DataLinkFrame {
//         DataLinkFrame {
//             kind,
//             src_addr: 1,
//             dest_addr: 2,
//             flags: 0,
//             payload: Vec::from_slice(b"hello").unwrap(),
//         }
//     }

//     fn make_codec() -> DataLinkCodec<MockPhy> {
//         DataLinkCodec::new(MockPhy::new())
//     }

//     /// Minimal single-poll executor for async functions that don't yield.
//     fn block_on<F: Future>(mut f: F) -> F::Output {
//         static VTABLE: RawWakerVTable = RawWakerVTable::new(
//             |_| RawWaker::new(core::ptr::null(), &VTABLE),
//             |_| {},
//             |_| {},
//             |_| {},
//         );
//         let raw = RawWaker::new(core::ptr::null(), &VTABLE);
//         let waker = unsafe { Waker::from_raw(raw) };
//         let mut cx = Context::from_waker(&waker);
//         let mut future = unsafe { Pin::new_unchecked(&mut f) };
//         match future.as_mut().poll(&mut cx) {
//             Poll::Ready(v) => v,
//             Poll::Pending => panic!("future not ready"),
//         }
//     }

//     // ── encode / decode roundtrips ──

//     #[test]
//     fn encode_decode_roundtrip_direct() {
//         let frame = make_frame(FrameKind::Direct);
//         let mut buf = [0u8; 256];
//         let encoded = DataLinkCodec::<MockPhy>::encode(&frame, &mut buf).unwrap();
//         let decoded = DataLinkCodec::<MockPhy>::decode(encoded).unwrap();
//         assert_eq!(frame, decoded);
//     }

//     #[test]
//     fn encode_decode_roundtrip_transport() {
//         let frame = make_frame(FrameKind::Transport);
//         let mut buf = [0u8; 256];
//         let encoded = DataLinkCodec::<MockPhy>::encode(&frame, &mut buf).unwrap();
//         let decoded = DataLinkCodec::<MockPhy>::decode(encoded).unwrap();
//         assert_eq!(frame, decoded);
//     }

//     #[test]
//     fn empty_payload_roundtrip() {
//         let frame = DataLinkFrame {
//             kind: FrameKind::Transport,
//             src_addr: 0,
//             dest_addr: 0,
//             flags: 0,
//             payload: Vec::new(),
//         };
//         let mut buf = [0u8; 256];
//         let encoded = DataLinkCodec::<MockPhy>::encode(&frame, &mut buf).unwrap();
//         let decoded = DataLinkCodec::<MockPhy>::decode(encoded).unwrap();
//         assert_eq!(frame, decoded);
//     }

//     #[test]
//     fn different_kinds_produce_different_bytes() {
//         let direct = make_frame(FrameKind::Direct);
//         let transport = make_frame(FrameKind::Transport);
//         let mut buf1 = [0u8; 256];
//         let mut buf2 = [0u8; 256];
//         let enc1 = DataLinkCodec::<MockPhy>::encode(&direct, &mut buf1).unwrap();
//         let enc2 = DataLinkCodec::<MockPhy>::encode(&transport, &mut buf2).unwrap();
//         assert_ne!(enc1, enc2);
//     }

//     #[test]
//     fn same_frame_produces_same_bytes() {
//         let frame = make_frame(FrameKind::Direct);
//         let mut buf1 = [0u8; 256];
//         let mut buf2 = [0u8; 256];
//         let enc1 = DataLinkCodec::<MockPhy>::encode(&frame, &mut buf1).unwrap();
//         let enc2 = DataLinkCodec::<MockPhy>::encode(&frame, &mut buf2).unwrap();
//         assert_eq!(enc1, enc2);
//     }

//     // ── error cases ──

//     #[test]
//     fn decode_garbage_fails() {
//         let garbage = [0xFFu8; 10];
//         let result = DataLinkCodec::<MockPhy>::decode(&garbage);
//         assert!(result.is_err());
//     }

//     #[test]
//     fn decode_empty_fails() {
//         let result = DataLinkCodec::<MockPhy>::decode(&[]);
//         assert!(result.is_err());
//     }

//     // ── trait methods (async) ──

//     #[test]
//     fn try_send_frame_encodes_to_phy() {
//         let mut codec = make_codec();
//         let frame = make_frame(FrameKind::Direct);

//         block_on(codec.try_send_frame(frame.clone())).unwrap();

//         // Decode what mock phy received
//         let decoded = DataLinkCodec::<MockPhy>::decode(&codec.phy.sent).unwrap();
//         assert_eq!(frame, decoded);
//     }

//     #[test]
//     fn try_recv_frame_decodes_from_phy() {
//         let mut codec = make_codec();
//         let frame = make_frame(FrameKind::Transport);

//         // Pre-load phy with encoded bytes
//         let mut enc_buf = [0u8; 256];
//         let encoded = DataLinkCodec::<MockPhy>::encode(&frame, &mut enc_buf).unwrap();
//         codec.phy.set_recv(encoded);

//         let mut rx_buf = [0u8; 256];
//         let decoded = block_on(codec.try_recv_frame(&mut rx_buf)).unwrap();
//         assert_eq!(frame, decoded);
//     }

//     #[test]
//     fn send_recv_roundtrip() {
//         let mut codec = make_codec();
//         let frame = make_frame(FrameKind::Transport);

//         // Send
//         block_on(codec.try_send_frame(frame.clone())).unwrap();

//         // Feed sent bytes into receive side
//         codec.phy.set_recv(&codec.phy.sent.clone());

//         // Recv
//         let mut rx_buf = [0u8; 256];
//         let decoded = block_on(codec.try_recv_frame(&mut rx_buf)).unwrap();
//         assert_eq!(frame, decoded);
//     }
// }

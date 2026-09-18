use crate::proto_impl::link::frame::Frame;

use super::error::LinkError;
use sat_core::comm::address::Address;
use sat_core::comm::link::LinkLayer;
use sat_core::comm::phy::PhyLayer;

pub struct LinkImpl<P: PhyLayer> {
    addr: Address,
    phy: P,
    buf: [u8; 255],
}

impl<P: PhyLayer> LinkImpl<P> {
    pub fn new(addr: Address, phy: P) -> Self {
        Self {
            addr,
            phy,
            buf: [0u8; 255],
        }
    }
}

impl<P: PhyLayer> LinkLayer for LinkImpl<P> {
    type Error = LinkError;

    async fn send_frame(&mut self, dst: Address, frame: &[u8]) -> Result<(), Self::Error> {
        let frame = Frame {
            src: self.addr.0,
            dst: dst.0,
            payload: frame,
        };

        let payload =
            postcard::to_slice(&frame, &mut self.buf).map_err(|_| LinkError::PayloadTooLarge)?;
        self.phy
            .send_bytes(payload)
            .await
            .map_err(|_| LinkError::Phy)?;
        Ok(())
    }

    async fn recv_frame<'a>(
        &mut self,
        buf: &'a mut [u8],
    ) -> Result<(Address, &'a [u8]), Self::Error> {
        let (n, src) = loop {
            let n = self.phy.recv_bytes(buf).await.map_err(|_| LinkError::Phy)?;
            let frame: Frame = match postcard::from_bytes(&buf[..n]) {
                Ok(frame) => frame,
                // Битый кадр — как CRC fail в классических линках: дропаем,
                // ждём следующий. dst у него всё равно не прочитать.
                Err(_) => continue,
            };
            if Address(frame.dst) == self.addr {
                break (n, frame.src);
            }
        };
        // Эти же байты уже декодировались в цикле — повторный разбор инфаллибилен.
        let frame: Frame = postcard::from_bytes(&buf[..n]).expect("frame decoded in loop above");
        Ok((Address(src), frame.payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sat_core::comm::address::BROADCAST_ADDRESS;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::vec::Vec;

    use core::future::Future;
    use core::task::{Context, Poll};
    use std::task::{Wake, Waker};

    // --- std executor, без внешних зависимостей ---
    struct ThreadWaker(std::thread::Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: Future>(fut: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut fut = core::pin::pin!(fut);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    // --- Mock PHY ---
    #[derive(Debug)]
    struct MockPhyError;

    #[derive(Default)]
    struct MockPhy {
        rx: VecDeque<Result<Vec<u8>, MockPhyError>>, // wire-кадры и ошибки на приём
        tx: Vec<Vec<u8>>,                            // отправленные кадры
    }

    impl MockPhy {
        fn push_rx(&mut self, bytes: Vec<u8>) {
            self.rx.push_back(Ok(bytes));
        }

        fn push_rx_error(&mut self) {
            self.rx.push_back(Err(MockPhyError));
        }
    }

    impl PhyLayer for MockPhy {
        type Error = MockPhyError;

        async fn send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error> {
            self.tx.push(payload.to_vec());
            Ok(())
        }

        async fn recv_bytes<'a>(&mut self, buf: &'a mut [u8]) -> Result<usize, Self::Error> {
            let frame = self.rx.pop_front().ok_or(MockPhyError)??;
            assert!(frame.len() <= buf.len(), "mock frame больше буфера");
            buf[..frame.len()].copy_from_slice(&frame);
            Ok(frame.len())
        }
    }

    // --- helpers ---
    fn wire_frame(src: u32, dst: u32, payload: &[u8]) -> Vec<u8> {
        let mut buf = [0u8; 255];
        postcard::to_slice(&Frame { src, dst, payload }, &mut buf)
            .unwrap()
            .to_vec()
    }

    const OUR: Address = Address(1);

    #[test]
    fn send_frame_encodes_postcard_frame() {
        let mut link = LinkImpl::new(OUR, MockPhy::default());

        block_on(link.send_frame(Address(2), &[0xAA, 0xBB])).unwrap();

        assert_eq!(link.phy.tx.len(), 1);
        assert_eq!(link.phy.tx[0], wire_frame(1, 2, &[0xAA, 0xBB]));
        let frame: Frame = postcard::from_bytes(&link.phy.tx[0]).unwrap();
        assert_eq!((frame.src, frame.dst), (1, 2));
        assert_eq!(frame.payload, &[0xAA, 0xBB]);
    }

    #[test]
    fn send_frame_does_not_leak_stale_bytes() {
        let mut link = LinkImpl::new(OUR, MockPhy::default());

        block_on(link.send_frame(Address(2), &[0xFF; 32])).unwrap();
        block_on(link.send_frame(Address(3), &[0x01])).unwrap();

        assert_eq!(link.phy.tx.len(), 2);
        assert_eq!(link.phy.tx[1], wire_frame(1, 3, &[0x01]));
    }

    #[test]
    fn recv_frame_returns_own_frame() {
        let mut phy = MockPhy::default();
        phy.push_rx(wire_frame(7, 1, &[1, 2, 3]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let (src, payload) = block_on(link.recv_frame(&mut buf)).unwrap();

        assert_eq!(src, Address(7));
        assert_eq!(payload, &[1, 2, 3]);
    }

    #[test]
    fn recv_frame_skips_foreign_addresses() {
        let mut phy = MockPhy::default();
        phy.push_rx(wire_frame(7, 99, &[9]));
        phy.push_rx(wire_frame(8, 100, &[8]));
        phy.push_rx(wire_frame(7, 1, &[1, 2, 3]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let (src, payload) = block_on(link.recv_frame(&mut buf)).unwrap();

        assert_eq!(src, Address(7));
        assert_eq!(payload, &[1, 2, 3]);
        assert!(link.phy.rx.is_empty());
    }

    #[test]
    fn recv_frame_ignores_broadcast_currently() {
        // Документирует текущее поведение: BROADCAST_ADDRESS фильтруется.
        let mut phy = MockPhy::default();
        phy.push_rx(wire_frame(7, BROADCAST_ADDRESS.0, &[9]));
        phy.push_rx(wire_frame(7, 1, &[1]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let (_, payload) = block_on(link.recv_frame(&mut buf)).unwrap();

        assert_eq!(payload, &[1]);
    }

    #[test]
    fn recv_frame_payload_borrows_caller_buf() {
        let mut phy = MockPhy::default();
        phy.push_rx(wire_frame(7, 1, &[1, 2, 3, 4]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let buf_start = buf.as_ptr() as usize;
        let buf_end = buf_start + buf.len();

        let (_, payload) = block_on(link.recv_frame(&mut buf)).unwrap();

        let ptr = payload.as_ptr() as usize;
        assert!(
            ptr > buf_start && ptr < buf_end,
            "payload должен быть срезом внутри buf вызывающего"
        );
    }

    #[test]
    fn recv_frame_empty_payload() {
        let mut phy = MockPhy::default();
        phy.push_rx(wire_frame(7, 1, &[]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let (src, payload) = block_on(link.recv_frame(&mut buf)).unwrap();

        assert_eq!(src, Address(7));
        assert!(payload.is_empty());
    }

    #[test]
    fn send_frame_rejects_oversize_payload() {
        let mut link = LinkImpl::new(OUR, MockPhy::default());

        let err = block_on(link.send_frame(Address(2), &[0u8; 255])).unwrap_err();

        assert_eq!(err, LinkError::PayloadTooLarge);
        assert!(link.phy.tx.is_empty());
    }

    #[test]
    fn recv_frame_propagates_phy_error() {
        let mut phy = MockPhy::default();
        phy.push_rx_error();
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let err = block_on(link.recv_frame(&mut buf)).unwrap_err();

        assert_eq!(err, LinkError::Phy);
    }

    #[test]
    fn recv_frame_skips_corrupted_frames() {
        let mut phy = MockPhy::default();
        phy.push_rx(std::vec![0xFF, 0xFF, 0xFF]); // не postcard
        phy.push_rx(wire_frame(7, 1, &[1, 2, 3]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let (src, payload) = block_on(link.recv_frame(&mut buf)).unwrap();

        assert_eq!(src, Address(7));
        assert_eq!(payload, &[1, 2, 3]);
        assert!(link.phy.rx.is_empty());
    }

    #[test]
    fn send_recv_roundtrip_between_two_links() {
        let mut a = LinkImpl::new(Address(1), MockPhy::default());
        let mut b = LinkImpl::new(Address(2), MockPhy::default());

        block_on(a.send_frame(Address(2), &[0xDE, 0xAD])).unwrap();
        let wire = a.phy.tx.pop().unwrap();
        b.phy.push_rx(wire);

        let mut buf = [0u8; 255];
        let (src, payload) = block_on(b.recv_frame(&mut buf)).unwrap();

        assert_eq!(src, Address(1));
        assert_eq!(payload, &[0xDE, 0xAD]);
    }
}

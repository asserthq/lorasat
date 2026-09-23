use crate::proto_impl::link::frame::{BeaconFrame, DataFrame, Frame};

use super::error::LinkError;
use sat_core::comm::address::{Address, BROADCAST_ADDRESS};
use sat_core::comm::link::{LinkLayer, RecvFrame};
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

    async fn send_phy(&mut self, frame: Frame<'_>) -> Result<(), LinkError> {
        let payload =
            postcard::to_slice(&frame, &mut self.buf).map_err(|_| LinkError::PayloadTooLarge)?;
        self.phy
            .send_bytes(payload)
            .await
            .map_err(|_| LinkError::Phy)?;
        Ok(())
    }
}

impl<P: PhyLayer> LinkLayer for LinkImpl<P> {
    type Error = LinkError;

    async fn send_beacon(&mut self, beacon: &[u8]) -> Result<(), Self::Error> {
        let frame = Frame::Beacon(BeaconFrame { payload: beacon });
        self.send_phy(frame).await
    }

    async fn send_frame(&mut self, dst: Address, data: &[u8]) -> Result<(), Self::Error> {
        let frame = Frame::Data(DataFrame {
            src: self.addr.0,
            dst: dst.0,
            payload: data,
        });

        self.send_phy(frame).await
    }

    async fn recv_frame<'a>(&mut self, buf: &'a mut [u8]) -> Result<RecvFrame<'a>, Self::Error> {
        let n = loop {
            let n = self.phy.recv_bytes(buf).await.map_err(|_| LinkError::Phy)?;
            let frame: Frame = match postcard::from_bytes(&buf[..n]) {
                Ok(frame) => frame,
                Err(_) => continue,
            };
            match &frame {
                Frame::Beacon(_) => break n,
                Frame::Data(data) => {
                    let dst = Address(data.dst);
                    if dst == self.addr || dst == BROADCAST_ADDRESS {
                        break n;
                    }
                }
            }
        };

        let frame: Frame = postcard::from_bytes(&buf[..n]).expect("frame decoded in loop above");
        let recv = match frame {
            Frame::Data(data_frame) => RecvFrame::Data {
                src: Address(data_frame.src),
                payload: data_frame.payload,
            },
            Frame::Beacon(beacon_frame) => RecvFrame::Beacon {
                payload: beacon_frame.payload,
            },
        };
        Ok(recv)
    }

    fn addr(&self) -> Address {
        self.addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto_impl::link::frame::BeaconFrame;
    use sat_core::comm::address::BROADCAST_ADDRESS;
    use sat_core::comm::link::RecvFrame;
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
        let frame = Frame::Data(DataFrame { src, dst, payload });
        postcard::to_slice(&frame, &mut buf).unwrap().to_vec()
    }

    fn wire_beacon(payload: &[u8]) -> Vec<u8> {
        let mut buf = [0u8; 255];
        let frame = Frame::Beacon(BeaconFrame { payload });
        postcard::to_slice(&frame, &mut buf).unwrap().to_vec()
    }

    const OUR: Address = Address(1);

    #[test]
    fn send_frame_encodes_postcard_frame() {
        let mut link = LinkImpl::new(OUR, MockPhy::default());

        block_on(link.send_frame(Address(2), &[0xAA, 0xBB])).unwrap();

        assert_eq!(link.phy.tx.len(), 1);
        assert_eq!(link.phy.tx[0], wire_frame(1, 2, &[0xAA, 0xBB]));
        let frame: Frame = postcard::from_bytes(&link.phy.tx[0]).unwrap();
        match frame {
            Frame::Data(data) => {
                assert_eq!((data.src, data.dst), (1, 2));
                assert_eq!(data.payload, &[0xAA, 0xBB]);
            }
            Frame::Beacon(_) => panic!("expected Data frame, got Beacon"),
        }
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
        let recv = block_on(link.recv_frame(&mut buf)).unwrap();

        match recv {
            RecvFrame::Data { src, payload } => {
                assert_eq!(src, Address(7));
                assert_eq!(payload, &[1, 2, 3]);
            }
            RecvFrame::Beacon { .. } => panic!("expected Data, got Beacon"),
        }
    }

    #[test]
    fn recv_frame_skips_foreign_addresses() {
        let mut phy = MockPhy::default();
        phy.push_rx(wire_frame(7, 99, &[9]));
        phy.push_rx(wire_frame(8, 100, &[8]));
        phy.push_rx(wire_frame(7, 1, &[1, 2, 3]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let recv = block_on(link.recv_frame(&mut buf)).unwrap();

        match recv {
            RecvFrame::Data { src, payload } => {
                assert_eq!(src, Address(7));
                assert_eq!(payload, &[1, 2, 3]);
            }
            RecvFrame::Beacon { .. } => panic!("expected Data, got Beacon"),
        }
        assert!(link.phy.rx.is_empty());
    }

    #[test]
    fn recv_frame_accepts_broadcast() {
        // Broadcast-кадры (адрес 0xFFFFFFFF) доходят до каждого узла.
        let mut phy = MockPhy::default();
        phy.push_rx(wire_frame(7, BROADCAST_ADDRESS.0, &[9]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let recv = block_on(link.recv_frame(&mut buf)).unwrap();

        match recv {
            RecvFrame::Data { payload, .. } => assert_eq!(payload, &[9]),
            RecvFrame::Beacon { .. } => panic!("expected Data, got Beacon"),
        }
    }

    #[test]
    fn recv_frame_still_rejects_foreign_unicast() {
        // Чужой unicast-адрес отбрасывается, broadcast проходит.
        let mut phy = MockPhy::default();
        phy.push_rx(wire_frame(7, 99, &[5]));
        phy.push_rx(wire_frame(8, 1, &[1, 2, 3]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let recv = block_on(link.recv_frame(&mut buf)).unwrap();

        match recv {
            RecvFrame::Data { src, payload } => {
                assert_eq!(src, Address(8));
                assert_eq!(payload, &[1, 2, 3]);
            }
            RecvFrame::Beacon { .. } => panic!("expected Data, got Beacon"),
        }
    }

    #[test]
    fn recv_frame_payload_borrows_caller_buf() {
        let mut phy = MockPhy::default();
        phy.push_rx(wire_frame(7, 1, &[1, 2, 3, 4]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let buf_start = buf.as_ptr() as usize;
        let buf_end = buf_start + buf.len();

        let recv = block_on(link.recv_frame(&mut buf)).unwrap();

        let payload = match recv {
            RecvFrame::Data { payload, .. } => payload,
            RecvFrame::Beacon { payload } => payload,
        };
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
        let recv = block_on(link.recv_frame(&mut buf)).unwrap();

        match recv {
            RecvFrame::Data { src, payload } => {
                assert_eq!(src, Address(7));
                assert!(payload.is_empty());
            }
            RecvFrame::Beacon { .. } => panic!("expected Data, got Beacon"),
        }
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
        let recv = block_on(link.recv_frame(&mut buf)).unwrap();

        match recv {
            RecvFrame::Data { src, payload } => {
                assert_eq!(src, Address(7));
                assert_eq!(payload, &[1, 2, 3]);
            }
            RecvFrame::Beacon { .. } => panic!("expected Data, got Beacon"),
        }
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
        let recv = block_on(b.recv_frame(&mut buf)).unwrap();

        match recv {
            RecvFrame::Data { src, payload } => {
                assert_eq!(src, Address(1));
                assert_eq!(payload, &[0xDE, 0xAD]);
            }
            RecvFrame::Beacon { .. } => panic!("expected Data, got Beacon"),
        }
    }

    // --- beacon tests ---

    #[test]
    fn send_beacon_encodes_postcard_beacon_frame() {
        let mut link = LinkImpl::new(OUR, MockPhy::default());

        block_on(link.send_beacon(&[0xBE, 0xAC])).unwrap();

        assert_eq!(link.phy.tx.len(), 1);
        assert_eq!(link.phy.tx[0], wire_beacon(&[0xBE, 0xAC]));
        let frame: Frame = postcard::from_bytes(&link.phy.tx[0]).unwrap();
        match frame {
            Frame::Beacon(b) => assert_eq!(b.payload, &[0xBE, 0xAC]),
            Frame::Data(_) => panic!("expected Beacon, got Data"),
        }
    }

    #[test]
    fn recv_beacon_always_accepted() {
        let mut phy = MockPhy::default();
        phy.push_rx(wire_beacon(&[0xCA, 0xFE]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let recv = block_on(link.recv_frame(&mut buf)).unwrap();

        match recv {
            RecvFrame::Beacon { payload } => assert_eq!(payload, &[0xCA, 0xFE]),
            RecvFrame::Data { .. } => panic!("expected Beacon, got Data"),
        }
    }

    #[test]
    fn recv_beacon_interleaved_with_data() {
        // Маяк принимается даже среди чужих data-кадров.
        let mut phy = MockPhy::default();
        phy.push_rx(wire_frame(7, 99, &[1])); // чужой — пропускается
        phy.push_rx(wire_beacon(&[0xBE]));
        let mut link = LinkImpl::new(OUR, phy);

        let mut buf = [0u8; 255];
        let recv = block_on(link.recv_frame(&mut buf)).unwrap();

        match recv {
            RecvFrame::Beacon { payload } => assert_eq!(payload, &[0xBE]),
            RecvFrame::Data { .. } => panic!("expected Beacon, got Data"),
        }
    }

    #[test]
    fn send_beacon_rejects_oversize_payload() {
        let mut link = LinkImpl::new(OUR, MockPhy::default());

        let err = block_on(link.send_beacon(&[0u8; 255])).unwrap_err();

        assert_eq!(err, LinkError::PayloadTooLarge);
        assert!(link.phy.tx.is_empty());
    }

    #[test]
    fn beacon_data_roundtrip() {
        // Маяк и data-кадр корректно проходят через один канал.
        let mut a = LinkImpl::new(Address(10), MockPhy::default());
        let mut b = LinkImpl::new(Address(20), MockPhy::default());

        // a отправляет маяк
        block_on(a.send_beacon(&[0xB0])).unwrap();
        let beacon_wire = a.phy.tx.pop().unwrap();

        // a отправляет data кадр
        block_on(a.send_frame(Address(20), &[0xD0])).unwrap();
        let data_wire = a.phy.tx.pop().unwrap();

        // b принимает маяк
        b.phy.push_rx(beacon_wire);
        let mut buf = [0u8; 255];
        let recv = block_on(b.recv_frame(&mut buf)).unwrap();
        match recv {
            RecvFrame::Beacon { payload } => assert_eq!(payload, &[0xB0]),
            RecvFrame::Data { .. } => panic!("expected Beacon, got Data"),
        }

        // b принимает data
        b.phy.push_rx(data_wire);
        let mut buf = [0u8; 255];
        let recv = block_on(b.recv_frame(&mut buf)).unwrap();
        match recv {
            RecvFrame::Data { src, payload } => {
                assert_eq!(src, Address(10));
                assert_eq!(payload, &[0xD0]);
            }
            RecvFrame::Beacon { .. } => panic!("expected Data, got Beacon"),
        }
    }
}

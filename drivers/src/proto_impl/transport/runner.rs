use defmt::info;
use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_sync::mutex::Mutex;
use heapless::{LinearMap, Vec};
use sat_core::comm::endpoint::Endpoint;
use sat_core::comm::link::RecvFrame;
use sat_core::comm::{Address, LinkLayer};

use super::transport::SharedSocketsMap;

use crate::proto_impl::transport::error::TransportError;
use crate::proto_impl::transport::pdu::Packet;
use crate::proto_impl::transport::socket::SocketImpl;

use super::pdu::Pdu;

pub type TxFrame = (Address, &[u8]);

type RxToSocketSender = Sender<NoopRawMutex, RecvFrame, 8>;
type SocketToTxReceiver = Receiver<NoopRawMutex, TxFrame, 8>;
pub type SharedSocketsMap = Mutex<CriticalSectionRawMutex, LinearMap<u8, RxToSocketSender, 4>>;

pub struct TransportRunner<L: LinkLayer> {
    link: L,
    sockets: SharedSocketsMap,
    tx_queue: SocketToTxReceiver,
}

impl<L: LinkLayer> TransportRunner<L> {
    pub fn new(link: L, sockets: SharedSocketsMap, tx_queue: SocketToTxReceiver) -> Self {
        Self {
            link,
            sockets,
            tx_queue,
        }
    }

    pub async fn run(&mut self) {
        let mut buf = [0u8; 255];
        loop {
            let tx = self.tx_queue.receive();
            let rx = self.link.recv_frame(&mut buf);

            match select(tx, rx).await {
                Either::First(tx_frame) => self.send(tx_frame).await.unwrap(),
                Either::Second(rx_frame) => self.dispatch(rx_frame.unwrap()).await.unwrap(),
            }
        }
    }

    async fn dispatch(&mut self, frame: RecvFrame) -> Result<(), TransportError> {
        match frame {
            RecvFrame::Beacon { payload } => {
                todo!()
            }
            RecvFrame::Data { src, payload } => {
                todo!()
            }
        }
    }

    async fn dispatch_pkt(&mut self, pkt: Packet) -> Result<(), TransportError> {
        todo!()
    }

    async fn handle_connect(&mut self, pkt: Packet) -> Result<(), TransportError> {
        todo!()
    }

    async fn send(&mut self, tx_frame: TxFrame) -> Result<(), TransportError> {
        let (addr, frame) = tx_frame;
        self.link
            .send_frame(addr, frame)
            .await
            .map_err(|_| TransportError::Link)
    }

    fn parse(payload: &[u8]) -> Result<Packet, TransportError> {
        let pkt = postcard::from_bytes(&payload).map_err(|_| TransportError::Decode)?;
        Ok(pkt)
    }
}

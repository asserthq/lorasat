use embassy_sync::channel::{Receiver, Sender};
use embassy_sync::mutex::NoopRawMutex;
use sat_core::comm::Socket;
use sat_core::comm::endpoint::Endpoint;
use sat_core::comm::{Address, Socket};

use crate::proto_impl::transport::TransportError;
use crate::proto_impl::transport::runner::TxFrame;
use crate::proto_impl::transport::stack::TransportStack;

pub type RxToSocketReceiver = Receiver<NoopRawMutex, &[u8], 8>;
pub type TxSender = Sender<NoopRawMutex, TxFrame, 8>;

#[derive(Debug, Default)]
enum SocketState {
    #[default]
    Unconnected,
    Connected(Endpoint),
}

pub struct SocketImpl {
    port: u8,
    rx: RxToSocketReceiver,
    tx: SocketToTxSender,
    state: SocketState,
}

impl SocketImpl {
    pub fn new(port: u8, rx: RxToSocketReceiver, tx: TxSender) -> Self {
        Self {
            port,
            rx,
            tx,
            state: SocketState::default(),
        }
    }

    pub fn connect(&mut self, ep: Endpoint) -> Result<(), TransportError> {
        todo!()
    }

    pub fn accept(&mut self, port: u8) -> Result<(), TransportError> {
        todo!()
    }
}

impl Socket for SocketImpl {
    type Error = TransportError;

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        if let SocketState::Connected(ep) = &self.state {
            let tx_frame = (ep, data);
            self.tx.send(tx_frame).await?;
            Ok(())
        } else {
            Err(TransportError::NotConnected)
        }
    }

    async fn recv<'a>(&'a mut self, buf: &'a mut [u8]) -> Result<&'a [u8], Self::Error> {
        self.rx.recv(buf).await?;
        Ok(buf)
    }

    fn port(&self) -> u8 {
        self.port
    }
}

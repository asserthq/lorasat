use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::{Channel, Receiver, Sender},
};
use sat_core::comm::endpoint::Endpoint;

use crate::proto_impl::transport::{
    error::TransportError,
    socket::{RxToSocketReceiver, SocketImpl, TxSender},
    transport::SharedSocketsMap,
};

pub struct TransportStack {
    sockets: SharedSocketsMap,
    tx_sender: TxSender,
}

impl TransportStack {
    pub fn new(sockets: SharedSocketsMap, tx_sender: TxSender) -> Self {
        Self { sockets, tx_sender }
    }

    pub async fn socket(&mut self, port: u8, ep: Endpoint) -> Result<SocketImpl, TransportError> {
        let rx_receiver = self.register_socket(port).await;
        let socket = SocketImpl::new(port, rx_receiver, self.tx_sender.clone());
        Ok(socket)
    }

    pub fn send_unrelieable(&mut self, ep: Endpoint, data: &[u8]) -> Result<(), TransportError> {
        todo!()
    }

    async fn register_socket(&mut self, port: u8) -> RxToSocketReceiver {
        let rx_channel = Channel::new();
        let (rx_sender, rx_receiver) = channel.split();
        let mut sockets = self.sockets.lock().await;
        sockets.insert(port, rx_sender);
        rx_receiver
    }
}

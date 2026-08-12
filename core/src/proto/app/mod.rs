pub mod error;

use core::fmt::Debug;

use crate::layer::app::{AppLayer, AppMessage};
use crate::layer::transport::{TransportLayer, TransportMessage};
use crate::proto::app::error::AppLayerImplError;
use heapless::Vec;

pub struct AppLayerImpl<T: TransportLayer + Debug> {
    transport: T,
    addr: u32,
}

impl<T: TransportLayer + Debug> AppLayerImpl<T> {
    pub fn new(transport: T, addr: u32) -> Self {
        Self { transport, addr }
    }
}

impl<T: TransportLayer + Debug> AppLayer for AppLayerImpl<T> {
    type Error = AppLayerImplError<T>;

    async fn try_send_message(&mut self, msg: AppMessage) -> Result<(), Self::Error> {
        let mut buf = [0u8; 4096];
        let ser = postcard::to_slice(&msg, &mut buf).map_err(AppLayerImplError::from)?;
        let payload = Vec::from_slice(ser).map_err(|_| AppLayerImplError::Error)?;
        let transport_msg = TransportMessage {
            dest_addr: 0u32,
            payload,
        };
        self.transport
            .try_send_message(transport_msg)
            .await
            .map_err(AppLayerImplError::Transport)
    }

    async fn try_recv_message<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> Result<AppMessage, Self::Error> {
        let transport_msg = self
            .transport
            .try_recv_message(buf)
            .await
            .map_err(AppLayerImplError::Transport)?;
        postcard::from_bytes(&transport_msg.payload).map_err(AppLayerImplError::from)
    }
}

use super::DataLinkLayer;
use super::frame::Frame;
use crate::physical::PhysicalLayer;

pub struct DataLinkCodec<P: PhysicalLayer> {
    phy: P,
}

impl<P: PhysicalLayer> DataLinkCodec<P> {
    pub fn new(phy: P) -> Self {
        Self { phy }
    }
}

impl<P: PhysicalLayer> DataLinkLayer for DataLinkCodec<P> {
    type Error = crate::error::Error;

    async fn try_send_frame(&mut self, frame: Frame) -> Result<(), Self::Error> {
        todo!()
    }

    async fn try_recv_frame(&mut self, buf: &mut [u8]) -> Result<Frame, Self::Error> {
        todo!()
    }
}

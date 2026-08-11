use core::fmt::Debug;

use crate::physical::PhysicalLayer;

use crate::data_link::DataLinkLayer;
use crate::data_link::data::Data;
use crate::data_link::frame::Frame;

use super::codec_error::CodecError;

pub struct DataLinkCodec<P: PhysicalLayer + Debug> {
    phy: P,
}

impl<P: PhysicalLayer + Debug> DataLinkCodec<P> {
    pub fn new(phy: P) -> Self {
        Self { phy }
    }

    fn encode_data<'a>(data: &Data, buf: &'a mut [u8]) -> Result<&'a mut [u8], CodecError<P>> {
        postcard::to_slice(data, buf).map_err(CodecError::from)
    }

    fn decode_data(raw: &[u8]) -> Result<Data, CodecError<P>> {
        postcard::from_bytes(raw).map_err(CodecError::from)
    }

    fn encode_frame<'a>(frame: &Frame, buf: &'a mut [u8]) -> Result<&'a mut [u8], CodecError<P>> {
        postcard::to_slice(frame, buf).map_err(CodecError::from)
    }

    fn decode_frame(raw: &[u8]) -> Result<Frame, CodecError<P>> {
        postcard::from_bytes(raw).map_err(CodecError::from)
    }
}

impl<P: PhysicalLayer + Debug> DataLinkLayer for DataLinkCodec<P> {
    type Error = CodecError<P>;

    async fn try_send_frame(&mut self, frame: Frame) -> Result<(), Self::Error> {
        let mut buf = [0u8; 256];
        let payload = Self::encode_frame(&frame, &mut buf)?;
        self.phy
            .try_send_bytes(payload)
            .await
            .map_err(|e| CodecError::<P>::Physical(e))
    }

    async fn try_recv_frame(&mut self, buf: &mut [u8]) -> Result<Frame, Self::Error> {
        let payload = self
            .phy
            .try_recv_bytes(buf)
            .await
            .map_err(|e| CodecError::<P>::Physical(e))?;
        Self::decode_frame(payload)
    }
}

pub mod error;

pub use error::Error;

use embedded_io_async::{Read, Write};
use sat_core::layer::phy::PhyLayer;

pub struct Uart<T> {
    inner: T,
}

impl<T> Uart<T>
where
    T: Read + Write,
{
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> PhyLayer for Uart<T>
where
    T: Read + Write,
{
    type Error = Error;

    async fn send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error> {
        self.inner
            .write_all(payload)
            .await
            .map_err(|_| Error::Write)
    }

    async fn recv_bytes<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        let mut n = 0usize;

        loop {
            if n >= buf.len() {
                return Err(Error::FrameTooLong);
            }

            let mut byte = [0u8; 1];
            let read = self.inner.read(&mut byte).await.map_err(|_| Error::Read)?;
            if read == 0 {
                return Err(Error::UnexpectedEof);
            }

            buf[n] = byte[0];
            n += 1;

            if byte[0] == 0 {
                return Ok(&mut buf[..n]);
            }
        }
    }
}

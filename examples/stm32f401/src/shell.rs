use core::fmt;

use embedded_io_async::{Read, Write};

#[derive(Debug, Clone, PartialEq, Eq, defmt::Format)]
pub enum ShellError {
    Read,
    Write,
    UnexpectedEof,
}

impl fmt::Display for ShellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellError::Read => f.write_str("shell read error"),
            ShellError::Write => f.write_str("shell write error"),
            ShellError::UnexpectedEof => f.write_str("stream ended before line terminator"),
        }
    }
}

pub struct Shell<T> {
    inner: T,
    last_was_cr: bool,
}

impl<T> Shell<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            last_was_cr: false,
        }
    }

    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T> Shell<T>
where
    T: Read + Write,
{
    pub async fn read_line(&mut self, buf: &mut [u8]) -> Result<usize, ShellError> {
        let mut len = 0usize;

        loop {
            let mut byte = [0u8; 1];
            let n = self
                .inner
                .read(&mut byte)
                .await
                .map_err(|_| ShellError::Read)?;
            if n == 0 {
                return Err(ShellError::UnexpectedEof);
            }

            let b = byte[0];

            if b == b'\r' {
                self.last_was_cr = true;
                self.inner
                    .write_all(b"\r\n")
                    .await
                    .map_err(|_| ShellError::Write)?;
                return Ok(len);
            }

            if b == b'\n' {
                if self.last_was_cr {
                    self.last_was_cr = false;
                    continue;
                }
                self.inner
                    .write_all(b"\r\n")
                    .await
                    .map_err(|_| ShellError::Write)?;
                return Ok(len);
            }

            self.last_was_cr = false;

            if b == 0x08 || b == 0x7F {
                if len > 0 {
                    len -= 1;
                    self.inner
                        .write_all(b"\x08 \x08")
                        .await
                        .map_err(|_| ShellError::Write)?;
                }
                continue;
            }

            if len >= buf.len() {
                continue;
            }

            buf[len] = b;
            len += 1;
            self.inner
                .write_all(&byte)
                .await
                .map_err(|_| ShellError::Write)?;
        }
    }
}

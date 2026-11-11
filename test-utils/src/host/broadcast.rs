use core::fmt;

use sat_csp::CspInterface;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct IdealBroadcastMedium {
    sender: broadcast::Sender<Vec<u8>>,
}

impl IdealBroadcastMedium {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "broadcast medium capacity must be non-zero");
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn attach(&self) -> IdealBroadcastInterface {
        IdealBroadcastInterface {
            sender: self.sender.clone(),
            receiver: self.sender.subscribe(),
        }
    }
}

pub struct IdealBroadcastInterface {
    sender: broadcast::Sender<Vec<u8>>,
    receiver: broadcast::Receiver<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdealBroadcastError {
    NoReceivers,
    Closed,
    Lagged(u64),
    FrameTooLarge { frame: usize, buffer: usize },
}

impl fmt::Display for IdealBroadcastError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoReceivers => formatter.write_str("broadcast medium has no receivers"),
            Self::Closed => formatter.write_str("broadcast medium is closed"),
            Self::Lagged(skipped) => {
                write!(formatter, "broadcast receiver lagged by {skipped} frames")
            }
            Self::FrameTooLarge { frame, buffer } => {
                write!(
                    formatter,
                    "frame requires {frame} bytes; buffer has {buffer}"
                )
            }
        }
    }
}

impl std::error::Error for IdealBroadcastError {}

impl CspInterface for IdealBroadcastInterface {
    type Error = IdealBroadcastError;

    async fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        self.sender
            .send(frame.to_vec())
            .map(|_| ())
            .map_err(|_| IdealBroadcastError::NoReceivers)
    }

    async fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error> {
        let received = self.receiver.recv().await.map_err(|error| match error {
            broadcast::error::RecvError::Closed => IdealBroadcastError::Closed,
            broadcast::error::RecvError::Lagged(skipped) => IdealBroadcastError::Lagged(skipped),
        })?;

        if received.len() > frame.len() {
            return Err(IdealBroadcastError::FrameTooLarge {
                frame: received.len(),
                buffer: frame.len(),
            });
        }

        frame[..received.len()].copy_from_slice(&received);
        Ok(received.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_attached_interface_receives_each_frame() {
        let medium = IdealBroadcastMedium::new(4);
        let mut first = medium.attach();
        let mut second = medium.attach();
        let mut third = medium.attach();

        first.send(b"radio frame").await.unwrap();

        for interface in [&mut first, &mut second, &mut third] {
            let mut buffer = [0; 32];
            let length = interface.receive(&mut buffer).await.unwrap();
            assert_eq!(&buffer[..length], b"radio frame");
        }
    }

    #[tokio::test]
    async fn short_buffer_discards_the_complete_frame() {
        let medium = IdealBroadcastMedium::new(2);
        let mut sender = medium.attach();
        let mut receiver = medium.attach();

        sender.send(b"too large").await.unwrap();

        let mut buffer = [0xa5; 4];
        assert_eq!(
            receiver.receive(&mut buffer).await,
            Err(IdealBroadcastError::FrameTooLarge {
                frame: 9,
                buffer: 4,
            })
        );
        assert_eq!(buffer, [0xa5; 4]);
    }
}

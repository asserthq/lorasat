#[derive(Debug)]
pub enum Error<CommE> {
    Comm(CommE),
    InvalidId(u8),
    Timeout,
}

impl<CommE> From<CommE> for Error<CommE> {
    fn from(e: CommE) -> Self {
        Self::Comm(e)
    }
}

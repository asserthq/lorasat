pub trait Logger {
    type Error;

    fn append(&mut self, data: &[u8]) -> Result<(), Self::Error>;

    fn flush(&mut self) -> Result<(), Self::Error>;
}

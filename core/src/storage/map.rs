use core::fmt::Debug;

#[derive(Clone, Copy, Debug, Eq, PartialEq, defmt::Format)]
pub struct Key(pub u16);

#[allow(async_fn_in_trait)]
pub trait MapStorage {
    type Error: Debug;

    async fn store(&mut self, key: Key, value: &[u8]) -> Result<(), Self::Error>;

    async fn load(&mut self, key: Key, buffer: &mut [u8]) -> Result<Option<usize>, Self::Error>;

    async fn remove(&mut self, key: Key) -> Result<bool, Self::Error>;
}

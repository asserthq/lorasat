use core::fmt::Debug;

#[derive(Clone, Copy, Debug, Eq, PartialEq, defmt::Format)]
pub struct ConfigKey(pub u16);

/// Key-value storage for persistent configuration values.
///
/// Configuration is not modeled as a FIFO ring: storing an existing key
/// replaces its previous value. Values remain opaque bytes so each subsystem
/// can keep its codec and schema versioning outside the storage backend.
#[allow(async_fn_in_trait)]
pub trait KeyValueStorage {
    type Error: Debug;

    async fn store(&mut self, key: ConfigKey, value: &[u8]) -> Result<(), Self::Error>;

    async fn load(
        &mut self,
        key: ConfigKey,
        buffer: &mut [u8],
    ) -> Result<Option<usize>, Self::Error>;

    async fn remove(&mut self, key: ConfigKey) -> Result<bool, Self::Error>;
}

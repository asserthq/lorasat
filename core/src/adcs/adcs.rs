use super::{AdcsCommand, AdcsEvent};

#[allow(async_fn_in_trait)]
pub trait AdcSystem {
    type Error;
    async fn apply_cmd(&mut self, cmd: AdcsCommand) -> Result<(), Self::Error>;
    async fn next_event(&self) -> Result<AdcsEvent, Self::Error>;
}

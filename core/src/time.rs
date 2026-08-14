use core::time::Duration;

pub trait Clock {
    fn now_micros(&self) -> u64;
}

#[allow(async_fn_in_trait)]
pub trait Delay {
    async fn delay(&mut self, dur: Duration);
}

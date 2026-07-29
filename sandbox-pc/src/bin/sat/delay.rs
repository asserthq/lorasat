use std::time::Duration;

#[allow(async_fn_in_trait)]
pub trait Delay {
    async fn after(&mut self, duration: Duration);
}

// ── tokio impl ──
pub struct TokioDelay;

impl Delay for TokioDelay {
    async fn after(&mut self, d: Duration) {
        tokio::time::sleep(d).await;
    }
}

// ── embassy impl ──
// pub struct EmbassyDelay;
// impl Delay for EmbassyDelay {
//     async fn after(&mut self, d: Duration) {
//         embassy_time::Timer::after(d).await;
//     }
// }

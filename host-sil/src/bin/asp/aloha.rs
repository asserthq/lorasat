use std::time::Duration;

use tokio::time::Instant;

use crate::random::Random;

pub const SLOT_MS: u64 = 100;
pub const BEACON_GUARD_MS: u64 = 2_000;

#[derive(Clone, Copy)]
pub struct Slot {
    pub index: u32,
    pub count: u32,
    pub opens_at: Instant,
    pub closes_at: Instant,
}

pub fn choose_slot(received_at: Instant, interval_sec: u16, random: &mut Random) -> Option<Slot> {
    let interval_ms = u64::from(interval_sec) * 1_000;
    let usable_ms = interval_ms.saturating_sub(BEACON_GUARD_MS);
    let count = (usable_ms / SLOT_MS) as u32;
    if count == 0 {
        return None;
    }

    let index = random.next_u32() % count;
    let opens_at = received_at + Duration::from_millis(u64::from(index) * SLOT_MS);
    Some(Slot {
        index,
        count,
        opens_at,
        closes_at: opens_at + Duration::from_millis(SLOT_MS),
    })
}

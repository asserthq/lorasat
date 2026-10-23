use core::sync::atomic::{AtomicU32, Ordering};

use defmt::info;
use embassy_time::Instant;

static SEED: AtomicU32 = AtomicU32::new(1);

pub fn chip_uid() -> u32 {
    let uid = unsafe {
        let base = 0x1FFFF7AC as *const u32;
        let u0 = core::ptr::read_volatile(base);
        let u1 = core::ptr::read_volatile(base.add(1));
        let u2 = core::ptr::read_volatile(base.add(2));
        u0 ^ u1.rotate_left(11) ^ u2.rotate_left(22)
    };
    if uid == 0 { 1 } else { uid }
}

pub fn init_seed() {
    let uid = chip_uid();

    let systick = unsafe { core::ptr::read_volatile(0xE000E018 as *const u32) };

    let mut acc: u32 = 0;
    for i in 0u32..16 {
        let s = unsafe { core::ptr::read_volatile(0xE000E018 as *const u32) };
        acc = acc.rotate_left(7) ^ s.wrapping_mul(i.wrapping_add(1));
        for _ in 0..(i * 7 + 3) {
            core::hint::spin_loop();
        }
    }

    let t = Instant::now().as_ticks() as u32;

    let mut seed = uid ^ systick ^ acc ^ t.wrapping_mul(0x9E37_79B9);
    if seed == 0 {
        seed = 1;
    }
    SEED.store(seed, Ordering::Relaxed);
    info!(
        "RNG seed = 0x{:08x} (uid=0x{:08x}, systick=0x{:08x}, acc=0x{:08x})",
        seed, uid, systick, acc
    );
}

pub fn rand_u32() -> u32 {
    let mut x = SEED.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    SEED.store(x, Ordering::Relaxed);
    x
}

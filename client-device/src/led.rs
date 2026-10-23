use core::sync::atomic::{AtomicBool, Ordering};

use embassy_stm32::gpio::Output;
use embassy_time::Timer;

pub static BLINK_FLAG: AtomicBool = AtomicBool::new(false);

#[embassy_executor::task]
pub async fn led_task(mut led_g: Output<'static>, mut led_r: Output<'static>) {
    led_r.set_low();
    led_g.set_high();

    loop {
        if BLINK_FLAG.load(Ordering::Relaxed) {
            BLINK_FLAG.store(false, Ordering::Relaxed);

            led_r.set_high();
            led_g.set_low();
            Timer::after_secs(1).await;

            led_g.set_high();
            led_r.set_low();
        }
        Timer::after_millis(20).await;
    }
}

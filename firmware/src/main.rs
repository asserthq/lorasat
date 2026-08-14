#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::Timer;
use firmware_lib as _;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("blinky start");

    // PC13 — built-in LED на Black Pill F401
    // активный низкий: Low = горит, High = не горит
    let mut led = Output::new(p.PC13, Level::High, Speed::Low);

    loop {
        led.set_low();
        Timer::after_millis(250).await;

        led.set_high();
        Timer::after_millis(250).await;
    }
}

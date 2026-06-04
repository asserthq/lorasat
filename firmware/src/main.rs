#![no_std]
#![no_main]

use firmware_lib as _;
use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::Timer;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let pers = embassy_stm32::init(Default::default());
    info!("Hello World!");

    let mut led = Output::new(pers.PE13, Level::High, Speed::Low);

    loop {
        info!("high");
        led.set_high();
        Timer::after_millis(1000).await;

        info!("low");
        led.set_low();
        Timer::after_millis(1000).await;
    }
}

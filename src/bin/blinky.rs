#![no_main]
#![no_std]

use sat_fsw as _; // global logger + panicking-behavior + memory layout

use stm32f1xx_hal as hal;

use cortex_m_rt::entry;
use hal::pac;
use hal::prelude::*;

#[entry]
fn main() -> ! {
    let dp = pac::Peripherals::take().unwrap();

    let mut rcc = dp.RCC.constrain();
    let mut gpioc = dp.GPIOC.split(&mut rcc);

    let mut led = gpioc.pc13.into_push_pull_output(&mut gpioc.crh);

    led.set_low();

    loop {
        led.toggle();
        defmt::info!(
            "toggle {}",
            match led.is_set_high() {
                true => "high",
                false => "low",
            }
        );
        cortex_m::asm::delay(8_000_000);
    }
}

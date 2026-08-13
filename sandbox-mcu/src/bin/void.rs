#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt::info;
use sandbox_lib as _;

#[entry]
fn main() -> ! {
    let x = 7u8;
    info!("wfijfifsskf {}", x);

    let x = x * x;
    info!("hf379952io4hrf {}", x);

    loop {
        cortex_m::asm::nop();
    }

    sandbox_lib::exit()
}

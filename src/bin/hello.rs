#![no_main]
#![no_std]

use sat_fsw as _; // global logger + panicking-behavior + memory layout

#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::println!("Hello, world!");

    sat_fsw::exit()
}

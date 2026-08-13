#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt::*;
use firmware_lib as _;

#[entry]
fn main() -> ! {
    debug!("helo shpiga");
    firmware_lib::exit()
}

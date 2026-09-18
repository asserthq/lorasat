#![no_std]
#![no_main]

use defmt::info;

use embassy_executor::Spawner;

use sandbox_lib::{self as _, exit};

use mmc5983_rs::Mmc5983;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    let mut mag = Mmc5983::new_with_i2c(i2c);

    // Initialize the device
    mag.init()?;

    // Optional: Calibrate offset
    let offset = mag.calibrate_offset(&mut delay)?;

    // Read magnetic field (one-shot mode)
    let field = mag.magnetic_field()?;
    println!(
        "Magnetic field: X={} Y={} Z={} Gauss",
        field.x_gauss(),
        field.y_gauss(),
        field.z_gauss()
    );

    // Read temperature
    let temp = mag.temperature()?;
    println!("Temperature: {}°C", temp.degrees_celsius());

    exit()
}

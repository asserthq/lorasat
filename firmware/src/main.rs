#![no_std]
#![no_main]

use embassy_executor::Spawner;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    firmware_lib::run(spawner).await
}

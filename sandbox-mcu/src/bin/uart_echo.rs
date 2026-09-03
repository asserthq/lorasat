#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::usart::{self, BufferedUart, Config};
use embassy_stm32::{bind_interrupts, peripherals};

use embedded_io_async::{Read, Write};

use sandbox_lib as _;

bind_interrupts!(struct Irqs {
    USART2 => usart::BufferedInterruptHandler<peripherals::USART2>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("uart echo start");

    let mut config = Config::default();
    config.baudrate = 115_200;

    let mut tx_buf = [0u8; 128];
    let mut rx_buf = [0u8; 128];

    let mut uart = BufferedUart::new(
        p.USART2,
        p.PA3,
        p.PA2,
        &mut tx_buf,
        &mut rx_buf,
        Irqs,
        config,
    )
    .unwrap();

    let _ = uart.write_all(b"echo ready\r\n").await;

    let mut byte = [0u8; 1];
    loop {
        if uart.read(&mut byte).await.is_ok() {
            info!("rx: {=u8}", byte[0]);
            let _ = uart.write_all(&byte).await;
        }
    }
}

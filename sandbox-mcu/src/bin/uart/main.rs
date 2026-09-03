#![no_std]
#![no_main]

use defmt::info;

use embedded_io_async::Write;

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::usart::{self, BufferedUart, Config};
use embassy_stm32::{bind_interrupts, peripherals};

use sandbox_lib as _;
use sandbox_lib::shell::Shell;

mod commands;

bind_interrupts!(struct Irqs {
    USART2 => usart::BufferedInterruptHandler<peripherals::USART2>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    info!("UART SHELL START");

    let mut led = Output::new(p.PC13, Level::High, Speed::Low);

    let mut config = Config::default();
    config.baudrate = 115_200;

    let mut tx_buf = [0u8; 128];
    let mut rx_buf = [0u8; 128];

    let uart = BufferedUart::new(
        p.USART2,
        p.PA3,
        p.PA2,
        &mut tx_buf,
        &mut rx_buf,
        Irqs,
        config,
    )
    .unwrap();

    let mut shell = Shell::new(uart);

    shell
        .inner_mut()
        .write_all(b"sat shell ready\r\n")
        .await
        .unwrap();
    info!("ENTERING SHELL LOOP");

    let mut line = [0u8; 128];

    loop {
        shell.inner_mut().write_all(b"> ").await.unwrap();

        let len = match shell.read_line(&mut line).await {
            Ok(len) => len,
            Err(e) => {
                defmt::error!("read_line failed: {:?}", e);
                continue;
            }
        };

        if len > 0 {
            commands::process_command(shell.inner_mut(), &mut led, &line[..len]).await;
        }
    }
}

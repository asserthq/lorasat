#![no_std]
#![no_main]

use defmt::info;

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
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

    info!("UART SHELL START");

    let mut led = Output::new(p.PC13, Level::High, Speed::Low);

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

    info!("uart initialized");

    uart.write_all(b"sat shell ready\r\n").await.unwrap();

    info!("ENTERING SHELL LOOP");

    let mut command = [0u8; 128];
    let mut command_len = 0usize;

    let mut last_was_cr = false;

    uart.write_all(b"> ").await.unwrap();

    loop {
        let mut byte = [0u8; 1];

        match uart.read(&mut byte).await {
            Ok(0) => {
                continue;
            }

            Ok(_) => {
                info!("RX BYTE = {}", byte[0]);
            }

            Err(_) => {
                info!("UART READ ERROR");
                continue;
            }
        }

        let b = byte[0];

        if b == b'\r' {
            info!("ENTER PRESSED");
            info!("COMMAND LENGTH = {}", command_len);

            last_was_cr = true;

            uart.write_all(b"\r\n").await.unwrap();

            process_command(&mut uart, &mut led, &command[..command_len]).await;

            command_len = 0;

            uart.write_all(b"> ").await.unwrap();

            continue;
        }

        if b == b'\n' {
            if last_was_cr {
                last_was_cr = false;
                continue;
            }

            info!("ENTER PRESSED");
            info!("COMMAND LENGTH = {}", command_len);

            uart.write_all(b"\r\n").await.unwrap();

            process_command(&mut uart, &mut led, &command[..command_len]).await;

            command_len = 0;

            uart.write_all(b"> ").await.unwrap();

            continue;
        }

        last_was_cr = false;

        if b == 0x08 || b == 0x7F {
            if command_len > 0 {
                command_len -= 1;

                uart.write_all(b"\x08 \x08").await.unwrap();
            }

            continue;
        }

        if command_len < command.len() {
            command[command_len] = b;
            command_len += 1;

            uart.write_all(&byte).await.unwrap();
        } else {
            info!("COMMAND BUFFER FULL");
        }
    }
}

async fn process_command<W>(uart: &mut W, led: &mut Output<'_>, cmd: &[u8])
where
    W: Write,
{
    info!("COMMAND LENGTH = {}", cmd.len());

    if cmd.is_empty() {
        return;
    }

    if cmd == b"help" {
        info!("CMD = HELP");

        uart.write_all(b"commands:\r\n").await.unwrap();

        uart.write_all(b"help - show commands\r\n").await.unwrap();

        uart.write_all(b"ver - show version\r\n").await.unwrap();

        uart.write_all(b"led - toggle LED\r\n").await.unwrap();

        uart.write_all(b"led on - turn LED on\r\n").await.unwrap();

        uart.write_all(b"led off - turn LED off\r\n").await.unwrap();

        uart.write_all(b"echo <text> - echo text\r\n")
            .await
            .unwrap();

        return;
    }

    if cmd == b"ver" {
        info!("CMD = VER");

        uart.write_all(b"sat-fsw v0.1\r\n").await.unwrap();

        return;
    }

    if cmd == b"led" {
        info!("CMD = LED");

        if led.is_set_low() {
            led.set_high();

            uart.write_all(b"led off\r\n").await.unwrap();
        } else {
            led.set_low();

            uart.write_all(b"led on\r\n").await.unwrap();
        }

        return;
    }

    if cmd == b"led on" {
        info!("CMD = LED ON");

        led.set_low();

        uart.write_all(b"led on\r\n").await.unwrap();

        return;
    }

    if cmd == b"led off" {
        info!("CMD = LED OFF");

        led.set_high();

        uart.write_all(b"led off\r\n").await.unwrap();

        return;
    }

    if cmd.starts_with(b"echo ") {
        info!("CMD = ECHO");

        let text = &cmd[5..];

        uart.write_all(text).await.unwrap();

        uart.write_all(b"\r\n").await.unwrap();

        return;
    }

    info!("CMD = UNKNOWN");

    uart.write_all(b"unknown command\r\n").await.unwrap();
}

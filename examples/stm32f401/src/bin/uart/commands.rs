use defmt::info;
use embedded_io_async::Write;

use embassy_stm32::gpio::Output;

pub async fn process_command<W>(uart: &mut W, led: &mut Output<'_>, cmd: &[u8])
where
    W: Write,
{
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
    } else if cmd == b"ver" {
        info!("CMD = VER");

        uart.write_all(b"sat-fsw v0.1\r\n").await.unwrap();
    } else if cmd == b"led" {
        info!("CMD = LED");

        if led.is_set_low() {
            led.set_high();
            uart.write_all(b"led off\r\n").await.unwrap();
        } else {
            led.set_low();
            uart.write_all(b"led on\r\n").await.unwrap();
        }
    } else if cmd == b"led on" {
        info!("CMD = LED ON");

        led.set_low();
        uart.write_all(b"led on\r\n").await.unwrap();
    } else if cmd == b"led off" {
        info!("CMD = LED OFF");

        led.set_high();
        uart.write_all(b"led off\r\n").await.unwrap();
    } else if cmd.starts_with(b"echo ") {
        info!("CMD = ECHO");

        let text = &cmd[5..];
        uart.write_all(text).await.unwrap();
        uart.write_all(b"\r\n").await.unwrap();
    } else {
        info!("CMD = UNKNOWN");

        uart.write_all(b"unknown command\r\n").await.unwrap();
    }
}

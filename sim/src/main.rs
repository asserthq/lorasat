use std::net::UdpSocket;

mod udp_adapter;

use udp_adapter::{recv_cobs_packet, send_cobs_packet};

/// Печатает классический hex-дамп: смещение, 16 hex-байт и ASCII-представление.
fn print_hex_dump(data: &[u8]) {
    for (i, chunk) in data.chunks(16).enumerate() {
        // Смещение
        print!("{:08x}  ", i * 16);
        // Hex-часть
        for (j, byte) in chunk.iter().enumerate() {
            print!("{:02x} ", byte);
            if j == 7 {
                print!(" "); // дополнительный пробел по середине
            }
        }
        // Дополняем пробелами, если строка неполная
        if chunk.len() < 16 {
            let padding = (16 - chunk.len()) * 3 + if chunk.len() <= 7 { 1 } else { 0 };
            print!("{:width$}", "", width = padding);
        }
        // ASCII-часть
        print!(" |");
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                print!("{}", *byte as char);
            } else {
                print!(".");
            }
        }
        println!("|");
    }
}

fn main() {
    // Слушаем все входящие UDP-дейтаграммы на локальном порту 6502
    let socket = UdpSocket::bind("127.0.0.1:6502").expect("Не удалось занять порт 6502");
    println!("Слушаю UDP-порт 6502...");

    let mut buf = [0u8; 2048]; // буфер под максимальный размер одного пакета
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, src)) => {
                println!("\n--- Получено {} байт от {} ---", n, src);
                print_hex_dump(&buf[..n]);
                // Если вы подозреваете COBS – раскомментируйте следующую строку
                // try_cobs_decode(&buf[..n]);
            }
            Err(e) => eprintln!("Ошибка приёма: {}", e),
        }
    }
}

use std::{net::UdpSocket, thread, time::Duration};

mod udp_adapter;

use udp_adapter::{recv_cobs_packet, send_cobs_packet};

fn main() {
    // Сервер занимает фиксированный порт
    let server = UdpSocket::bind("127.0.0.1:34254").expect("bind server");
    // Клиент – на любом свободном порту
    let client = UdpSocket::bind("127.0.0.1:0").expect("bind client");

    // Запускаем приём в отдельном потоке
    let handle = thread::spawn(move || {
        let data = recv_cobs_packet(&server);
        println!("Сервер получил: {:?}", String::from_utf8_lossy(&data));
    });

    // Небольшая пауза, чтобы серверный поток точно начал слушать
    thread::sleep(Duration::from_millis(10));

    // Отправляем пакет с COBS-кодированием
    let message = b"Hello, COBS!";
    send_cobs_packet(&client, "127.0.0.1:34254", message);
    println!(
        "Клиент отправил: {:?}",
        std::str::from_utf8(message).unwrap()
    );

    // Дожидаемся завершения приёмника
    handle.join().unwrap();
}

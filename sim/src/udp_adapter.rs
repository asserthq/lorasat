use std::net::UdpSocket;

/// Отправляет payload как COBS-фрейм через UDP.
/// COBS-фрейм = encode(payload) + 0x00
pub fn send_cobs_packet(socket: &UdpSocket, addr: &str, payload: &[u8]) {
    // Худший случай COBS: на каждый байт данных может добавиться один overhead-байт,
    // плюс 2 байта (начальный overhead и завершающий 0). Но проще взять 254 * ceil(len/254) + 2.
    // Для простоты используем payload.len() * 2 + 2, что гарантированно хватит.
    let max_encoded_len = payload.len() * 2 + 2;
    let mut encoded = vec![0u8; max_encoded_len];
    let n = cobs::encode(payload, &mut encoded);
    // n — количество записанных байт в encoded (без завершающего нуля)
    // Добавляем завершающий 0x00 для обозначения конца фрейма
    if n < encoded.len() {
        encoded[n] = 0;
        let frame = &encoded[..=n]; // включая завершающий ноль
        socket.send_to(frame, addr).expect("send_to failed");
    } else {
        panic!("COBS buffer overflow");
    }
}

/// Принимает один COBS-фрейм из UDP-сокета.
/// Возвращает раскодированные данные.
pub fn recv_cobs_packet(socket: &UdpSocket) -> Vec<u8> {
    let mut buf = [0u8; 1024];
    let (n, _src) = socket.recv_from(&mut buf).expect("recv_from failed");

    // Ищем завершающий ноль COBS-фрейма
    let delim_pos = buf[..n]
        .iter()
        .position(|&b| b == 0)
        .expect("No COBS delimiter (0x00) found");
    let encoded = &buf[..delim_pos]; // без нуля

    // Размер буфера для декодирования: гарантированно не больше encoded.len(),
    // но проще взять с запасом, например, тот же размер, что и encoded.
    let mut decoded = vec![0u8; encoded.len()];
    let report = cobs::decode(encoded, &mut decoded).expect("COBS decode failed");
    // report указывает количество полностью раскодированных байт
    decoded.truncate(report.frame_size());
    decoded
}

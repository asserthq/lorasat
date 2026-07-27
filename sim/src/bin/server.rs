use std::net::UdpSocket;

// Структура, соответствующая формату '<fifffff':
// float t, int cmd, float dt, float u, float w[3]
// (все поля по 4 байта, little‑endian)
#[allow(non_snake_case)]
struct DataBase {
    t: f32,
    cmd: i32,
    dt: f32,
    u: f32,
    w: [f32; 3],
}

const PACKED_SIZE: usize = 28; // 7 полей × 4 байта

// Команды (как в исходном C++ коде)
const CMD_INIT: i32 = 0;
const CMD_RUN: i32 = 1;
const CMD_FIN: i32 = 2;
// const CMD_ERROR: i32 = 3;

/// Упаковывает структуру в массив байт (<fifffff, little‑endian).
fn pack(db: &DataBase) -> [u8; PACKED_SIZE] {
    let mut buf = [0u8; PACKED_SIZE];
    buf[0..4].copy_from_slice(&db.t.to_le_bytes());
    buf[4..8].copy_from_slice(&db.cmd.to_le_bytes());
    buf[8..12].copy_from_slice(&db.dt.to_le_bytes());
    buf[12..16].copy_from_slice(&db.u.to_le_bytes());
    buf[16..20].copy_from_slice(&db.w[0].to_le_bytes());
    buf[20..24].copy_from_slice(&db.w[1].to_le_bytes());
    buf[24..28].copy_from_slice(&db.w[2].to_le_bytes());
    buf
}

/// Распаковывает срез байт в структуру. Возвращает None при ошибке.
fn unpack(data: &[u8]) -> Option<DataBase> {
    if data.len() < PACKED_SIZE {
        return None;
    }
    // little‑endian прямо как питоновский '<fifffff'
    let t = f32::from_le_bytes(data[0..4].try_into().ok()?);
    let cmd = i32::from_le_bytes(data[4..8].try_into().ok()?);
    let dt = f32::from_le_bytes(data[8..12].try_into().ok()?);
    let u = f32::from_le_bytes(data[12..16].try_into().ok()?);
    let wx = f32::from_le_bytes(data[16..20].try_into().ok()?);
    let wy = f32::from_le_bytes(data[20..24].try_into().ok()?);
    let wz = f32::from_le_bytes(data[24..28].try_into().ok()?);
    Some(DataBase {
        t,
        cmd,
        dt,
        u,
        w: [wx, wy, wz],
    })
}

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:2910")?;
    println!("Готов слушать UDP порт 2910 (Ctrl+C для выхода)...");

    let mut buf = [0u8; 40]; // как в исходном C++ (с запасом)
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, src)) => {
                println!("Принято {} байт от {}", n, src);
                if let Some(mut db) = unpack(&buf[..n]) {
                    // --- обработка команды (как в C++) ---
                    match db.cmd {
                        CMD_INIT => {
                            // Инициализация
                            println!("  INIT");
                        }
                        CMD_RUN => {
                            db.w[0] += 1.0; // имитация работы
                            println!("  RUN, w[0] стал {}", db.w[0]);
                        }
                        CMD_FIN => {
                            println!("  FIN");
                        }
                        _ => {}
                    }
                    // --- отправка обработанных данных обратно клиенту ---
                    let response = pack(&db);
                    socket.send_to(&response, src)?;
                } else {
                    eprintln!("Ошибка распаковки пакета");
                }
            }
            Err(e) => eprintln!("Ошибка приёма: {}", e),
        }
    }
}

use std::time::Duration;

use sat_core::radio::HalfDuplexTransceiver;

pub struct Satellite<R: HalfDuplexTransceiver + Send + Sync> {
    radio: R,
    beacon_count: u64,
    buf: [u8; 256],
    idle_ticks: u64,
}

impl<R: HalfDuplexTransceiver + Send + Sync + 'static> Satellite<R> {
    pub fn new(radio: R) -> Self {
        Self {
            radio,
            beacon_count: 0,
            buf: [0u8; 256],
            idle_ticks: 0,
        }
    }

    /// Главный цикл — приём + маяк каждые 10 сек + idle каждые 100 мс.
    pub async fn run(mut self) {
        let mut beacon = tokio::time::interval(Duration::from_secs(10));
        let mut idle = tokio::time::interval(Duration::from_millis(100));

        println!("[sat] online, listening");

        loop {
            tokio::select! {
                _ = self.receive_tm() => {}

                _ = beacon.tick() => {
                    self.send_beacon().await;
                }

                _ = idle.tick() => {
                    self.idle_ticks += 1;
                }
            }
        }
    }

    async fn send_beacon(&mut self) {
        let msg = format!("BEACON:{}", self.beacon_count);
        match self.radio.transmit(msg.as_bytes()).await {
            Err(e) => {
                eprintln!("[radio] tx error: {e:?}");
            }
            Ok(n) => {
                println!("[radio] tx ok: {n} bytes");
            }
        }
        self.beacon_count += 1;
    }

    /// Блокирующий приём — ждёт пакет от клиента.
    async fn receive_tm(&mut self) {
        match self.radio.receive(&mut self.buf).await {
            Ok(n) => {
                let msg = std::str::from_utf8(&self.buf[..n]).unwrap_or("?");
                println!("[radio] rx ok: {} bytes: {msg}", n);
            }
            Err(e) => eprintln!("[radio] rx error: {e:?}"),
        };
    }
}

use std::fmt::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sat_core::radio::HalfDuplexTransceiver;

pub struct ClientDevice<R: HalfDuplexTransceiver + Send + Sync> {
    radio: R,
    buf: [u8; 256],
    pending_telemetry: String,
    sample_count: u64,
    idle_ticks: u64,
    rng_state: u64,
}

impl<R: HalfDuplexTransceiver + Send + Sync + 'static> ClientDevice<R> {
    pub fn new(radio: R) -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        Self {
            radio,
            buf: [0u8; 256],
            pending_telemetry: String::with_capacity(256),
            sample_count: 0,
            idle_ticks: 0,
            rng_state: if seed == 0 { 1 } else { seed },
        }
    }

    /// Главный цикл: слушаем маяки, копим телеметрию с random интервалом, idle 100 мс.
    pub async fn run(mut self) {
        let first_tlm = Duration::from_secs(self.rand_range(3, 8));
        let mut tlm_timer = tokio::time::interval(first_tlm);
        let mut idle = tokio::time::interval(Duration::from_millis(100));

        println!("[client] online, listening");

        loop {
            tokio::select! {
                // Ждём маяк — при получении сбрасываем накопленную телеметрию
                _ = self.wait_beacon() => {
                    self.flush_telemetry().await;
                }

                // Случайный сбор телеметрии (interval нагоняет пропущенные тики)
                _ = tlm_timer.tick() => {
                    self.collect_telemetry();
                    let next = Duration::from_secs(self.rand_range(3, 8));
                    tlm_timer.reset_after(next);
                }

                // Счётчик простоя — плейсхолдер для будущих служебных задач
                _ = idle.tick() => {
                    self.idle_ticks += 1;
                }
            }
        }
    }

    /// Блокируется до получения любого пакета (ожидается маяк).
    async fn wait_beacon(&mut self) {
        match self.radio.receive(&mut self.buf).await {
            Ok(n) => {
                let msg = std::str::from_utf8(&self.buf[..n]).unwrap_or("?");
                println!("[radio] rx beacon: {msg}");
            }
            Err(e) => eprintln!("[radio] rx error: {e:?}"),
        }
    }

    /// Отправляет накопленную телеметрию и очищает буфер.
    async fn flush_telemetry(&mut self) {
        let tlm = if self.pending_telemetry.is_empty() {
            format!("TLM:{}:empty", self.sample_count)
        } else {
            format!("TLM:{}:{}", self.sample_count, self.pending_telemetry)
        };

        println!("[radio] tx {tlm}");
        match self.radio.transmit(tlm.as_bytes()).await {
            Ok(n) => println!("[radio] tx ok: {n} bytes"),
            Err(e) => eprintln!("[radio] tx error: {e:?}"),
        }

        self.pending_telemetry.clear();
    }

    /// Генерирует случайную телеметрию и добавляет в буфер.
    fn collect_telemetry(&mut self) {
        let temp = 20.0 + self.rand_float() * 15.0; // 20.0 .. 35.0 °C
        let bat = 3.3 + self.rand_float() * 0.7; // 3.3 .. 4.0 V
        let rssi = -((40 + self.rand_u64() % 51) as i32); // -90 .. -40 dBm

        if !self.pending_telemetry.is_empty() {
            self.pending_telemetry.push_str(" ; ");
        }
        let _ = write!(
            self.pending_telemetry,
            "[ t={temp:.1},b={bat:.2},r={rssi} ]"
        );

        self.sample_count += 1;
        println!(
            "[client] collect #{:<3} temp={temp:.1} bat={bat:.2} rssi={rssi}",
            self.sample_count
        );
    }

    // --- Minimal LCG RNG ---

    fn lcg_step(&mut self) -> u64 {
        self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345);
        self.rng_state
    }

    fn rand_u64(&mut self) -> u64 {
        self.lcg_step()
    }

    fn rand_float(&mut self) -> f32 {
        (self.lcg_step() & 0xFFFF) as f32 / 65536.0
    }

    fn rand_range(&mut self, min: u64, max: u64) -> u64 {
        min + (self.lcg_step() % (max - min))
    }
}

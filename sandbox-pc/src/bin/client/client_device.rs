use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::telemetry::Telemetry;
use sat_core::error::Error;
use sat_core::protocol::data_link::{Beacon, DataFrame, Frame, FrameType};
use sat_core::radio::HalfDuplexTransceiver;

const CLIENT_ADDR: u32 = 9002;

pub struct ClientDevice<R: HalfDuplexTransceiver> {
    radio: R,
    buf: [u8; 256],
    pending_telemetry: Vec<Telemetry>,
    sample_count: u64,
    idle_ticks: u64,
    rng_state: u64,
}

impl<R: HalfDuplexTransceiver> ClientDevice<R> {
    pub fn new(radio: R) -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        Self {
            radio,
            buf: [0u8; 256],
            pending_telemetry: Vec::with_capacity(16),
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
                res = self.wait_beacon() => {
                    match res {
                        Ok(beacon) => {
                            println!("[client] rx beacon: {beacon:?}");
                            match self.flush_telemetry(beacon.sat_addr).await {
                                Ok(_) => {
                                    println!("[client] tx telemetry {} packets", self.pending_telemetry.len());
                                }
                                Err(e) => {
                                    eprintln!("[client] tx telemetry error: {e:?}");
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[client] wait beacon error: {e:?}");
                        }
                    }
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
    async fn wait_beacon(&mut self) -> Result<Beacon, Error> {
        match self.radio.receive(&mut self.buf).await {
            Ok(n) => {
                println!("[client radio] rx ok {n} bytes");
                let frame = Frame::try_decode(&self.buf).unwrap();
                match frame {
                    Frame::Beacon(beacon) => Ok(beacon),
                    _ => Err(Error::Internal("not beacon")),
                }
            }
            Err(_) => Err(Error::Internal("radio rx error")),
        }
    }

    /// Отправляет накопленную телеметрию и очищает буфер.
    async fn flush_telemetry(&mut self, sat_addr: u32) -> Result<(), Error> {
        let frame = Frame::Data(self.create_tm_data(sat_addr));
        let payload_vec = frame.encode();
        let payload = &payload_vec.as_slice();
        println!("[client] tx data: {frame:?}");

        match self.radio.transmit(payload).await {
            Ok(n) => {
                println!("[client radio] tx ok {n} bytes");
                self.pending_telemetry.clear();
                Ok(())
            }
            Err(_) => Err(Error::Internal("radio tx error")),
        }
    }

    fn create_tm_data(&self, sat_addr: u32) -> DataFrame {
        DataFrame {
            frame_type: FrameType::DataAsp,
            src_addr: CLIENT_ADDR,
            dest_addr: sat_addr,
            flags: Default::default(),
            data: Telemetry::serialize_batch(&self.pending_telemetry),
        }
    }

    fn collect_telemetry(&mut self) {
        let tm = Telemetry {
            temp: 20.0 + self.rand_float() * 15.0,       // 20.0 .. 35.0 °C
            bat_voltage: 3.3 + self.rand_float() * 0.7,  // 3.3 .. 4.0 V
            rssi: -((40 + self.rand_u64() % 51) as i32), // -90 .. -40 dBm
        };

        self.pending_telemetry.push(tm.clone());

        self.sample_count += 1;
        println!("[client] collect tm #{}: {tm:?}", self.sample_count);
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

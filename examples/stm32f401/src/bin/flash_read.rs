#![no_std]
#![no_main]

use defmt::{error, info, warn};

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::time::mhz;
use embassy_stm32::{bind_interrupts, dma, peripherals, spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use sat_drivers::flash::ring::FlashRing;
use sat_drivers::flash::{CAPACITY, W25Q64};

use {defmt_rtt as _, panic_probe as _};

const RECORD_MARKER: u8 = 0x7E;
const RECORD_HEADER_LEN: usize = 1 + 4 + 4;

#[derive(serde::Deserialize, defmt::Format)]
struct Telemetry {
    node_addr: u32,
    sample_id: u32,
    beacon_timestamp: u64,
    temp: f32,
    bat_voltage: f32,
}

bind_interrupts!(struct Irqs {
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>; // SPI2 RX
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>; // SPI2 TX
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(config);

    info!("flash read start");

    let cs = Output::new(p.PB12, Level::High, Speed::High);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = mhz(1);

    let spi = spi::Spi::new(
        p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, cs, Delay).unwrap();

    let mut flash = W25Q64::new(spi, Delay);
    match flash.read_id() {
        Ok(id) => info!("flash: JEDEC ID {:02x} {:02x} {:02x}", id[0], id[1], id[2]),
        Err(e) => error!("flash: read_id failed: {:?}", e),
    }

    let mut ring = match FlashRing::new(flash, 0, CAPACITY) {
        Ok(r) => r,
        Err(e) => {
            error!("flash: ring init failed: {:?}", e);
            return;
        }
    };

    if let Err(e) = ring.recover() {
        error!("flash: recover failed: {:?}", e);
        return;
    }
    info!(
        "flash: recovered, head={} tail={}",
        ring.head(),
        ring.tail()
    );

    ring.rewind();
    let mut buf = [0u8; 4096];
    let mut count = 0usize;
    loop {
        match ring.read_next(&mut buf) {
            Ok(Some(rec)) => {
                print_record(count, rec.seq, rec.timestamp, rec.data);
                count += 1;
            }
            Ok(None) => break,
            Err(e) => {
                error!("flash: read failed: {:?}", e);
                break;
            }
        }
    }
    info!("flash: dump done, {} records", count);
}

fn print_record(idx: usize, seq: u32, ts: u64, data: &[u8]) {
    info!("[{}] seq={} ts={} len={}", idx, seq, ts, data.len());
    info!("[{}] raw={:?}", idx, data);

    if data.len() < RECORD_HEADER_LEN || data[0] != RECORD_MARKER {
        warn!("[{}] bad record header, skip decode", idx);
        return;
    }

    let src = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
    let when = u32::from_le_bytes([data[5], data[6], data[7], data[8]]);

    match postcard::from_bytes::<Telemetry>(&data[RECORD_HEADER_LEN..]) {
        Ok(tm) => info!("[{}] src=0x{:08x} when={}ms tm={:?}", idx, src, when, tm),
        Err(_) => info!(
            "[{}] src=0x{:08x} when={}ms payload(raw)={:?}",
            idx,
            src,
            when,
            &data[RECORD_HEADER_LEN..]
        ),
    }
}

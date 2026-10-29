#![no_std]
#![no_main]

use core::future::pending;

use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_futures::select::select6;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{Peripherals, bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Delay, Instant, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use heapless::Vec;

use firmware_lib as _;

use sat_csp::{
    MAX_NODE_ID, RDP_TRAILER_SIZE, RdpConfig, RdpResources, RunnerBuilder, StackResources,
};
use sat_drivers::lora::Radio1262;

const MTU: usize = 255;
const SATELLITE_ADDRESS: u16 = 1;

const BEACON_PORT: u8 = 10;
const CLIENT_DATA_PORT: u8 = 11;
const COMMAND_PORT: u8 = 20;

const BEACON_VERSION: u8 = 1;
const BEACON_SIZE: usize = 13;
const BEACON_INTERVAL_SECONDS: u16 = 10;

const DUMP_VERSION: u8 = 1;
const DUMP_COMMAND: &[u8] = b"dump";
const RDP_APPLICATION_MAX: usize = MTU - sat_csp::HEADER_SIZE - RDP_TRAILER_SIZE;

const MAX_RECORDS: usize = 8;
const MAX_RECORD_PAYLOAD: usize = 64;

const RADIO_433_FREQUENCY_HZ: u32 = 433_000_000;
const RADIO_868_FREQUENCY_HZ: u32 = 868_000_000;

struct StoredRecord {
    source: u16,
    source_port: u8,
    payload: Vec<u8, MAX_RECORD_PAYLOAD>,
}

type RecordStore = Vec<StoredRecord, MAX_RECORDS>;

bind_interrupts!(struct Irqs {
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>; // SPI1 RX, radio 433
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI1 TX, radio 433
    DMA1_STREAM3 => dma::InterruptHandler<peripherals::DMA1_CH3>; // SPI2 RX, radio 868
    DMA1_STREAM4 => dma::InterruptHandler<peripherals::DMA1_CH4>; // SPI2 TX, radio 868
    EXTI4 => exti::InterruptHandler<interrupt::typelevel::EXTI4>; // radio 433 DIO1
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>; // radio 433 BUSY + radio 868 DIO1
    EXTI15_10 => exti::InterruptHandler<interrupt::typelevel::EXTI15_10>; // radio 868 BUSY
});

fn init_peripherals() -> Peripherals {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    embassy_stm32::init(config)
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = init_peripherals();

    let radio_433 = {
        let nss = Output::new(p.PA4, Level::High, Speed::Low);
        let reset = Output::new(p.PB6, Level::High, Speed::Low);
        let irq_dio1 = ExtiInput::new(p.PB4, p.EXTI4, Pull::Up, Irqs);
        let irq_busy = ExtiInput::new(p.PB5, p.EXTI5, Pull::Up, Irqs);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(200);
        let spi = spi::Spi::new(
            p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH2, Irqs, spi_config,
        );
        let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();

        Radio1262::new(
            spi,
            reset,
            irq_dio1,
            irq_busy,
            None,
            None,
            Delay,
            RADIO_433_FREQUENCY_HZ,
        )
        .await
        .unwrap()
    };

    let radio_868 = {
        // TODO(board): temporary second-radio pinout. Replace after schematic is fixed.
        let nss = Output::new(p.PB12, Level::High, Speed::Low);
        let reset = Output::new(p.PA8, Level::High, Speed::Low);
        let irq_dio1 = ExtiInput::new(p.PA9, p.EXTI9, Pull::Up, Irqs);
        let irq_busy = ExtiInput::new(p.PA10, p.EXTI10, Pull::Up, Irqs);

        let mut spi_config = spi::Config::default();
        spi_config.frequency = khz(200);
        let spi = spi::Spi::new(
            p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH4, p.DMA1_CH3, Irqs, spi_config,
        );
        let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();

        Radio1262::new(
            spi,
            reset,
            irq_dio1,
            irq_busy,
            None,
            None,
            Delay,
            RADIO_868_FREQUENCY_HZ,
        )
        .await
        .unwrap()
    };

    info!("both SX1262 radios initialized");

    let stack_433 = StackResources::<MTU, 4, 1, 4>::new();
    let stack_868 = StackResources::<MTU, 4, 2, 4>::new();
    let rdp_433 = RdpResources::<MTU>::new();
    rdp_433.update_time(0);

    let mut csp_builder_433 = RunnerBuilder::new(SATELLITE_ADDRESS, radio_433, &stack_433).unwrap();
    let beacon_433_socket = csp_builder_433.bind(BEACON_PORT).unwrap();
    let gs_cmd_listener = csp_builder_433
        .listen_rdp(COMMAND_PORT, &rdp_433, RdpConfig::new(0x2000))
        .unwrap();
    let mut csp_runner_433 = csp_builder_433.build();

    let mut csp_builder_868 = RunnerBuilder::new(SATELLITE_ADDRESS, radio_868, &stack_868).unwrap();
    let beacon_868_socket = csp_builder_868.bind(BEACON_PORT).unwrap();
    let mut client_data_socket = csp_builder_868.bind(CLIENT_DATA_PORT).unwrap();
    let mut csp_runner_868 = csp_builder_868.build();

    let records = Mutex::<CriticalSectionRawMutex, RecordStore>::new(RecordStore::new());

    let beacon_loop = async {
        loop {
            let beacon = encode_beacon(Instant::now().as_secs());
            beacon_433_socket
                .send_to(MAX_NODE_ID, BEACON_PORT, &beacon)
                .await
                .unwrap();
            beacon_868_socket
                .send_to(MAX_NODE_ID, BEACON_PORT, &beacon)
                .await
                .unwrap();
            info!("beacon queued on 433 and 868 MHz");
            Timer::after_secs(u64::from(BEACON_INTERVAL_SECONDS)).await;
        }
    };

    let collect_client_data = async {
        loop {
            let datagram = client_data_socket.recv_from().await;
            let mut payload = Vec::<u8, MAX_RECORD_PAYLOAD>::new();
            if payload.extend_from_slice(datagram.payload()).is_err() {
                warn!(
                    "drop subscriber record from {}:{}: {} bytes exceeds limit",
                    datagram.source(),
                    datagram.source_port(),
                    datagram.payload().len()
                );
                continue;
            }

            let mut records = records.lock().await;
            let record = StoredRecord {
                source: datagram.source(),
                source_port: datagram.source_port(),
                payload,
            };
            if records.push(record).is_err() {
                warn!("drop subscriber record: store full");
                continue;
            }
            info!(
                "stored opaque record from {}:{}, total={}",
                datagram.source(),
                datagram.source_port(),
                records.len()
            );
        }
    };

    let command_service = async {
        let mut connection = gs_cmd_listener.accept().await.unwrap();
        info!("ground RDP connection accepted");
        let command = connection.recv().await;
        if command.payload() != DUMP_COMMAND {
            warn!("unknown ground command");
        }

        let mut response = [0_u8; RDP_APPLICATION_MAX];
        let (response_length, included) = {
            let records = records.lock().await;
            encode_dump(&records, &mut response)
        };
        connection.send(&response[..response_length]).await.unwrap();
        info!("sent {} opaque records to ground", included);

        // Initial RDP profile accepts one connection and one send per handle.
        pending::<()>().await;
    };

    let rdp_clock = async {
        let mut now_ms = 0_u32;
        loop {
            Timer::after_millis(10).await;
            now_ms = now_ms.wrapping_add(10);
            rdp_433.update_time(now_ms);
        }
    };

    info!("satellite CSP tasks started");
    let _ = select6(
        csp_runner_433.run(),
        csp_runner_868.run(),
        beacon_loop,
        collect_client_data,
        command_service,
        rdp_clock,
    )
    .await;
    panic!("satellite CSP task stopped");
}

fn encode_beacon(uptime_seconds: u64) -> [u8; BEACON_SIZE] {
    let mut output = [0; BEACON_SIZE];
    output[0] = BEACON_VERSION;
    output[1..3].copy_from_slice(&SATELLITE_ADDRESS.to_be_bytes());
    output[3..11].copy_from_slice(&uptime_seconds.to_be_bytes());
    output[11..13].copy_from_slice(&BEACON_INTERVAL_SECONDS.to_be_bytes());
    output
}

fn encode_dump(records: &RecordStore, output: &mut [u8; RDP_APPLICATION_MAX]) -> (usize, u8) {
    output[0] = DUMP_VERSION;
    output[1] = 0;
    output[2] = records.len() as u8;

    let mut length = 3;
    let mut included = 0_u8;
    for record in records {
        let payload_length = record.payload.len();
        let required = 4 + payload_length;
        if length + required > output.len() {
            break;
        }

        output[length..length + 2].copy_from_slice(&record.source.to_be_bytes());
        output[length + 2] = record.source_port;
        output[length + 3] = payload_length as u8;
        output[length + 4..length + required].copy_from_slice(&record.payload);
        length += required;
        included += 1;
    }
    output[1] = included;
    (length, included)
}

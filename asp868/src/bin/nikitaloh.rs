#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use heapless::Vec;

use sat_core::layer::app::Message;
use sat_core::layer::transport::{self, Packet, PacketHeader, TransportLayer};
use sat_core::message::client_data::{ClientData, MAX_CLIENT_DATA};
use sat_core::message::Beacon;
use sat_drivers::lora::Radio1262;
use sat_drivers::proto_impl::link::LinkImpl;
use sat_drivers::proto_impl::transport::SimpleTransport;

use {defmt_rtt as _, panic_probe as _};

const LORA_FREQ: u32 = 868_000_000;
const NODE_ADDR: u32 = 10;

static BLINK_FLAG: AtomicBool = AtomicBool::new(false);
static SEED: AtomicU32 = AtomicU32::new(1);

fn init_seed() {
    let uid = unsafe {
        let base = 0x1FFFF7AC as *const u32;
        let u0 = core::ptr::read_volatile(base);
        let u1 = core::ptr::read_volatile(base.add(1));
        let u2 = core::ptr::read_volatile(base.add(2));
        u0 ^ u1.rotate_left(11) ^ u2.rotate_left(22)
    };

    let systick = unsafe { core::ptr::read_volatile(0xE000E018 as *const u32) };

    let mut acc: u32 = 0;
    for i in 0u32..16 {
        let s = unsafe { core::ptr::read_volatile(0xE000E018 as *const u32) };
        acc = acc.rotate_left(7) ^ s.wrapping_mul(i.wrapping_add(1));
        for _ in 0..(i * 7 + 3) {
            core::hint::spin_loop();
        }
    }

    let t = embassy_time::Instant::now().as_ticks() as u32;

    let mut seed = uid ^ systick ^ acc ^ t.wrapping_mul(0x9E37_79B9);
    if seed == 0 {
        seed = 1;
    }
    SEED.store(seed, Ordering::Relaxed);
    info!(
        "RNG seed = 0x{:08x} (uid=0x{:08x}, systick=0x{:08x}, acc=0x{:08x})",
        seed, uid, systick, acc
    );
}

fn rand_u32() -> u32 {
    let mut x = SEED.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    SEED.store(x, Ordering::Relaxed);
    x
}

bind_interrupts!(struct Irqs {
    DMA1_CHANNEL4_5_6_7 => dma::InterruptHandler<peripherals::DMA1_CH4>,
                            dma::InterruptHandler<peripherals::DMA1_CH5>;
    EXTI4_15 => exti::InterruptHandler<interrupt::typelevel::EXTI4_15>;
});

#[embassy_executor::task]
async fn led_task(mut led_g: Output<'static>, mut led_r: Output<'static>) {
    led_r.set_low();
    led_g.set_high();

    loop {
        if BLINK_FLAG.load(Ordering::Relaxed) {
            BLINK_FLAG.store(false, Ordering::Relaxed);

            led_r.set_high();
            led_g.set_low();
            Timer::after_secs(1).await;

            led_g.set_high();
            led_r.set_low();
        }
        Timer::after_millis(20).await;
    }
}

async fn wait_beacon<T: TransportLayer>(transport: &mut T) -> Beacon {
    let mut buf = [0u8; transport::MAX_TRANSPORT_MESSAGE_PAYLOAD];

    loop {
        let pkt = match transport.recv_message(&mut buf).await {
            Ok(pkt) => pkt,
            Err(_) => {
                Timer::after_millis(100).await;
                continue;
            }
        };

        match postcard::from_bytes::<Message>(&pkt.payload) {
            Ok(Message::BeaconMsg(beacon)) => return beacon,
            Ok(_) => info!("ignored non-beacon message"),
            Err(_) => info!("decode failed"),
        }
    }
}

async fn send_client_data<T: TransportLayer>(
    transport: &mut T,
    dest: u32,
    data: &[u8],
) {
    let msg = Message::ClientDataMsg(ClientData {
        data: Vec::<u8, MAX_CLIENT_DATA>::from_slice(data).unwrap(),
    });

    let mut buf = [0u8; transport::MAX_TRANSPORT_MESSAGE_PAYLOAD];
    let ser = postcard::to_slice(&msg, &mut buf).unwrap();
    let payload = Vec::<u8, { transport::MAX_TRANSPORT_MESSAGE_PAYLOAD }>::from_slice(ser).unwrap();

    let pkt = Packet {
        header: PacketHeader { dest_addr: dest },
        payload,
    };

    info!("tx ClientDataMsg -> 0x{:08x}", dest);
    transport.send_message(pkt).await.unwrap();
    info!("tx done");
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    init_seed();

    info!("ASP ALOHA start");

    let led_g = Output::new(p.PB2, Level::High, Speed::Low);
    let led_r = Output::new(p.PB10, Level::High, Speed::Low);
    spawner.spawn(led_task(led_g, led_r).unwrap());

    let nss = Output::new(p.PB12, Level::High, Speed::Low);
    let reset = Output::new(p.PB8, Level::High, Speed::Low);
    let irq_dio1 = ExtiInput::new(p.PB6, p.EXTI6, Pull::Up, Irqs);
    let irq_busy = ExtiInput::new(p.PB7, p.EXTI7, Pull::Up, Irqs);

    let mut spi_cfg = spi::Config::default();
    spi_cfg.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI2, p.PB13, p.PB15, p.PB14, p.DMA1_CH5, p.DMA1_CH4, Irqs, spi_cfg,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    let radio = Radio1262::new(spi, reset, irq_dio1, irq_busy, None, None, Delay, LORA_FREQ)
        .await
        .unwrap();
    info!("LoRa init OK");

    let link = LinkImpl::new(radio);
    let mut transport = SimpleTransport::new(NODE_ADDR, link);

    let mut beacon_count: u32 = 0;

    loop {
        info!("Waiting for beacon...");

        let beacon = wait_beacon(&mut transport).await;

        beacon_count += 1;

        info!(
            "Beacon #{}: sat=0x{:08x} interval={}s ts={}",
            beacon_count, beacon.sat_addr, beacon.interval_sec, beacon.timestamp
        );

        let max_delay_ms = (beacon.interval_sec as u32).saturating_mul(1000);
        let delay_ms = if max_delay_ms > 0 {
            let r = rand_u32()
                .wrapping_add(beacon.timestamp.wrapping_mul(0x9E37_79B9))
                .wrapping_add(beacon_count.wrapping_mul(0x85EB_CA6B));
            r % max_delay_ms
        } else {
            0
        };

        info!("ALOHA delay = {} ms", delay_ms);

        Timer::after_millis(delay_ms as u64).await;

        BLINK_FLAG.store(true, Ordering::Relaxed);
        info!("TX slot fired at delay={}ms", delay_ms);

        let mut data = Vec::<u8, MAX_CLIENT_DATA>::new();
        data.extend_from_slice(&beacon_count.to_le_bytes()).unwrap();
        data.extend_from_slice(&beacon.timestamp.to_le_bytes()).unwrap();

        send_client_data(&mut transport, beacon.sat_addr, &data).await;

        Timer::after_millis(1500).await;
    }
}

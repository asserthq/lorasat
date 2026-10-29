#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::{
    join::join,
    select::{Either, select, select3},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Timer;
use sat_csp::{CspInterface, RdpConfig, RdpResources, RunnerBuilder, StackResources};
use {defmt_rtt as _, panic_probe as _};

const MTU: usize = 255;

struct Frame {
    bytes: [u8; MTU],
    length: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MemoryLinkError {
    FrameTooLarge,
}

struct MemoryLink<'a> {
    outgoing: &'a Signal<CriticalSectionRawMutex, Frame>,
    incoming: &'a Signal<CriticalSectionRawMutex, Frame>,
}

impl CspInterface for MemoryLink<'_> {
    type Error = MemoryLinkError;

    async fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        if bytes.len() > MTU {
            return Err(MemoryLinkError::FrameTooLarge);
        }
        let mut frame = Frame {
            bytes: [0; MTU],
            length: bytes.len(),
        };
        frame.bytes[..bytes.len()].copy_from_slice(bytes);
        self.outgoing.signal(frame);
        Ok(())
    }

    async fn receive(&mut self, output: &mut [u8]) -> Result<usize, Self::Error> {
        let frame = self.incoming.wait().await;
        if frame.length > output.len() {
            return Err(MemoryLinkError::FrameTooLarge);
        }
        output[..frame.length].copy_from_slice(&frame.bytes[..frame.length]);
        Ok(frame.length)
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let _peripherals = embassy_stm32::init(Default::default());
    let ground_to_satellite = Signal::new();
    let satellite_to_ground = Signal::new();

    let ground_link = MemoryLink {
        outgoing: &ground_to_satellite,
        incoming: &satellite_to_ground,
    };
    let satellite_link = MemoryLink {
        outgoing: &satellite_to_ground,
        incoming: &ground_to_satellite,
    };

    let ground_stack = StackResources::<MTU, 2, 1, 1>::new();
    let satellite_stack = StackResources::<MTU, 2, 1, 1>::new();
    let ground_rdp = RdpResources::<MTU>::new();
    let satellite_rdp = RdpResources::<MTU>::new();
    ground_rdp.update_time(0);
    satellite_rdp.update_time(0);

    let mut ground_builder = RunnerBuilder::new(1, ground_link, &ground_stack).unwrap();
    let client = ground_builder
        .rdp_client(&ground_rdp, RdpConfig::new(0x1000))
        .unwrap();
    let mut ground_runner = ground_builder.build();

    let mut satellite_builder = RunnerBuilder::new(2, satellite_link, &satellite_stack).unwrap();
    let listener = satellite_builder
        .listen_rdp(20, &satellite_rdp, RdpConfig::new(0x2000))
        .unwrap();
    let mut satellite_runner = satellite_builder.build();

    let application = async {
        let (ground_connection, satellite_connection) =
            join(client.connect(2, 20), listener.accept()).await;
        let mut ground_connection = ground_connection.unwrap();
        let mut satellite_connection = satellite_connection.unwrap();

        ground_connection.send(b"execute command").await.unwrap();
        let command = satellite_connection.recv().await;
        assert_eq!(command.payload(), b"execute command");
        info!("RDP command received and delivery acknowledged");
    };

    let clock = async {
        let mut now_ms = 0_u32;
        loop {
            Timer::after_millis(10).await;
            now_ms = now_ms.wrapping_add(10);
            ground_rdp.update_time(now_ms);
            satellite_rdp.update_time(now_ms);
        }
    };
    let background = select3(ground_runner.run(), satellite_runner.run(), clock);
    match select(application, background).await {
        Either::First(()) => info!("CSP RDP example finished"),
        Either::Second(_) => panic!("CSP runner stopped unexpectedly"),
    }

    loop {
        Timer::after_secs(60).await;
    }
}

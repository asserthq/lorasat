#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Timer;
use sat_csp::{Link, RunnerBuilder, StackResources};
use {defmt_rtt as _, panic_probe as _};

const MTU: usize = 255;

struct Frame {
    bytes: [u8; MTU],
    length: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoopbackError {
    FrameTooLarge,
}

struct LoopbackLink {
    frame: Signal<CriticalSectionRawMutex, Frame>,
}

impl LoopbackLink {
    const fn new() -> Self {
        Self {
            frame: Signal::new(),
        }
    }
}

impl Link for LoopbackLink {
    type Error = LoopbackError;

    async fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        if bytes.len() > MTU {
            return Err(LoopbackError::FrameTooLarge);
        }

        let mut frame = Frame {
            bytes: [0; MTU],
            length: bytes.len(),
        };
        frame.bytes[..bytes.len()].copy_from_slice(bytes);
        self.frame.signal(frame);
        Ok(())
    }

    async fn receive(&mut self, output: &mut [u8]) -> Result<usize, Self::Error> {
        let frame = self.frame.wait().await;
        if frame.length > output.len() {
            return Err(LoopbackError::FrameTooLarge);
        }

        output[..frame.length].copy_from_slice(&frame.bytes[..frame.length]);
        Ok(frame.length)
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let _peripherals = embassy_stm32::init(Default::default());
    let resources = StackResources::<MTU, 2, 1, 2>::new();

    let mut builder = RunnerBuilder::new(1, LoopbackLink::new(), &resources).unwrap();
    let mut socket = builder.bind(10).unwrap();
    let mut runner = builder.build();

    let application = async {
        socket.send_to(1, 10, b"hello loopback").await.unwrap();
        let datagram = socket.recv_from().await;

        assert_eq!(datagram.source(), 1);
        assert_eq!(datagram.source_port(), 10);
        assert_eq!(datagram.payload(), b"hello loopback");
        info!("CSP loopback datagram received");
    };

    match select(application, runner.run()).await {
        Either::First(()) => info!("CSP example finished"),
        Either::Second(_) => panic!("CSP runner stopped unexpectedly"),
    }

    loop {
        Timer::after_secs(60).await;
    }
}

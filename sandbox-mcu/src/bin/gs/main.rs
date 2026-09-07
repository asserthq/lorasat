#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::usart::{self, BufferedUart, Config};
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;

use sandbox_lib as _;
use sandbox_lib::shell::Shell;

use sat_drivers::lora::Radio;
use sat_drivers::proto_impl::link::LinkImpl;
use sat_drivers::proto_impl::transport::SimpleTransport;

mod commands;
mod task;

const LORA_FREQ_IN_HZ: u32 = 433_000_000;

bind_interrupts!(struct Irqs {
    USART2 => usart::BufferedInterruptHandler<peripherals::USART2>;
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>;
    DMA2_STREAM0 => dma::InterruptHandler<peripherals::DMA2_CH0>;
    EXTI0 => exti::InterruptHandler<interrupt::typelevel::EXTI0>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    let mut config = Config::default();
    config.baudrate = 115_200;

    let mut tx_buf = [0u8; 128];
    let mut rx_buf = [0u8; 128];

    let uart = BufferedUart::new(
        p.USART2,
        p.PA3,
        p.PA2,
        &mut tx_buf,
        &mut rx_buf,
        Irqs,
        config,
    )
    .unwrap();

    let shell = Shell::new(uart);

    let nss = Output::new(p.PA4, Level::High, Speed::Low);
    let reset = Output::new(p.PB1, Level::High, Speed::Low);
    let irq = ExtiInput::new(p.PB0, p.EXTI0, Pull::Up, Irqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH0, Irqs, spi_config,
    );
    let spi_device = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    let radio = Radio::new(spi_device, reset, irq, Delay, LORA_FREQ_IN_HZ)
        .await
        .unwrap();

    let link = LinkImpl::new(radio);
    let transport = SimpleTransport::new(0, link);

    task::gs_task(shell, transport).await;
}

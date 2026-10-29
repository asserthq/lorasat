#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, spi};
use embassy_time::{Delay, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use lora_phy::{
    LoRa,
    iv::GenericSx126xInterfaceVariant,
    mod_params::{Bandwidth, CodingRate, RxMode, SpreadingFactor},
    sx126x::{self, Sx126x, Sx1262, TcxoCtrlVoltage},
};
use {defmt_rtt as _, panic_probe as _};

const FREQUENCY_HZ: u32 = 433_000_000;
const MAX_PACKET_SIZE: usize = 255;

bind_interrupts!(struct Irqs {
    DMA2_STREAM2 => dma::InterruptHandler<peripherals::DMA2_CH2>; // SPI1 RX
    DMA2_STREAM3 => dma::InterruptHandler<peripherals::DMA2_CH3>; // SPI1 TX
    EXTI9_5 => exti::InterruptHandler<interrupt::typelevel::EXTI9_5>; // BUSY
    EXTI4 => exti::InterruptHandler<interrupt::typelevel::EXTI4>; // DIO1
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut clock_config = embassy_stm32::Config::default();
    clock_config.rcc.hsi = true;
    clock_config.rcc.sys = embassy_stm32::rcc::Sysclk::HSI;
    let p = embassy_stm32::init(clock_config);

    let nss = Output::new(p.PA4, Level::High, Speed::Low);
    let reset = Output::new(p.PB0, Level::High, Speed::Low);
    let busy = ExtiInput::new(p.PB1, p.EXTI5, Pull::Up, Irqs);
    let dio1 = ExtiInput::new(p.PB2, p.EXTI4, Pull::Up, Irqs);

    let mut spi_config = spi::Config::default();
    spi_config.frequency = khz(200);
    let spi = spi::Spi::new(
        p.SPI1, p.PA5, p.PA7, p.PA6, p.DMA2_CH3, p.DMA2_CH2, Irqs, spi_config,
    );
    let spi = ExclusiveDevice::new(spi, nss, Delay).unwrap();

    let interface = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None).unwrap();
    let radio_config = sx126x::Config {
        chip: Sx1262,
        tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V8),
        use_dcdc: true,
        rx_boost: true,
    };
    let mut lora = LoRa::new(Sx126x::new(spi, interface, radio_config), false, Delay)
        .await
        .unwrap();
    let modulation = lora
        .create_modulation_params(
            SpreadingFactor::_12,
            Bandwidth::_125KHz,
            CodingRate::_4_5,
            FREQUENCY_HZ,
        )
        .unwrap();
    let rx_params = lora
        .create_rx_packet_params(8, false, MAX_PACKET_SIZE as u8, true, false, &modulation)
        .unwrap();

    info!("SX1262 raw RX initialized at {} Hz", FREQUENCY_HZ);

    let mut buffer = [0_u8; MAX_PACKET_SIZE];
    loop {
        if let Err(error) = lora
            .prepare_for_rx(RxMode::Continuous, &modulation, &rx_params)
            .await
        {
            error!("prepare RX failed: {:?}", error);
            Timer::after_millis(250).await;
            continue;
        }

        let (length, status) = match lora.rx(&rx_params, &mut buffer).await {
            Ok(received) => received,
            Err(error) => {
                error!("RX failed: {:?}", error);
                Timer::after_millis(250).await;
                continue;
            }
        };
        let length = length as usize;

        info!(
            "RX length={} rssi={} snr={} bytes={:?}",
            length,
            status.rssi,
            status.snr,
            &buffer[..length]
        );
    }
}

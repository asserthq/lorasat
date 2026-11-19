use embassy_stm32::{exti::ExtiInput, gpio::Output, mode::Async};
use embassy_time::Delay;
use sat_drivers::lora::{RadioError, RadioSx1262 as GenericSx1262};

use crate::{
    board::{RadioParts, RadioSpiDevice, Sx1262Parts},
    config::radio,
};

pub(crate) type RadioSx1262 =
    GenericSx1262<RadioSpiDevice, Output<'static>, ExtiInput<'static, Async>, Delay>;

pub(crate) struct RadioDevices {
    pub platform: RadioSx1262,
    pub service: RadioSx1262,
}

pub(crate) async fn init(parts: RadioParts) -> Result<RadioDevices, RadioError> {
    let platform = init_one(parts.platform, radio::PLATFORM).await?;
    let service = init_one(parts.service, radio::SERVICE).await?;

    Ok(RadioDevices { platform, service })
}

async fn init_one(parts: Sx1262Parts, config: radio::Config) -> Result<RadioSx1262, RadioError> {
    RadioSx1262::new(
        parts.spi,
        parts.reset,
        parts.dio1,
        parts.busy,
        None,
        None,
        Delay,
        config.frequency_hz,
    )
    .await
}

use embassy_stm32::{exti::ExtiInput, gpio::Output, mode::Async};
use embassy_time::Delay;
use sat_drivers::lora::{RadioError, RadioSx1262 as GenericSx1262};

use crate::{
    board::{RadioParts, RadioSpiDevice},
    config::radio,
};

pub(crate) type RadioSx1262 =
    GenericSx1262<RadioSpiDevice, Output<'static>, ExtiInput<'static, Async>, Delay>;

pub(crate) struct Radios {
    pub platform: RadioSx1262,
    pub service: RadioSx1262,
}

pub(crate) async fn init(
    platform_parts: RadioParts,
    service_parts: RadioParts,
) -> Result<Radios, RadioError> {
    let platform = init_one(platform_parts, radio::PLATFORM).await?;
    let service = init_one(service_parts, radio::SERVICE).await?;

    Ok(Radios { platform, service })
}

async fn init_one(parts: RadioParts, config: radio::Config) -> Result<RadioSx1262, RadioError> {
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

use core::future::pending;
use defmt::{info, warn};

use embassy_executor::Spawner;

use crate::{
    board::{self, BoardParts, FlashParts, SensorsParts},
    channels::{self, WorkersHandles},
    devices::{adcs, radios},
    tasks,
};

pub async fn run(spawner: Spawner) -> ! {
    let BoardParts {
        platform_radio,
        service_radio,
        flash: FlashParts { spi: flash_spi },
        sensors: SensorsParts { i2c1, i2c2, i2c3 },
    } = board::init();

    let radios = radios::init(platform_radio, service_radio)
        .await
        .expect("SX1262 initialization failed");
    let adcs = adcs::init(i2c1)
        .await
        .expect("MMC5983MA initialization failed");

    info!("CDH, ADCS and EPS initialization complete");
    warn!("ADCS coil outputs are disabled until PWM parts are added to the BSP");

    let radios::Radios {
        platform: command_radio,
        service: service_radio,
    } = radios;
    let adcs::AdcsDevices {
        magnetometer,
        coils,
    } = adcs;
    let (controller_handles, worker_handles) = channels::split();
    let WorkersHandles {
        adcs: adcs_handle,
        eps: eps_handle,
        service: service_handle,
    } = worker_handles;

    // Retain resources whose tasks are not wired yet. Dropping their HAL
    // drivers would deinitialize the corresponding peripherals.
    let _reserved = (service_radio, service_handle, flash_spi, i2c2, i2c3);

    spawner.spawn(
        tasks::cdh::run(command_radio, controller_handles).expect("CDH task pool is available"),
    );
    spawner.spawn(
        tasks::adcs::run(magnetometer, coils, adcs_handle).expect("ADCS task pool is available"),
    );
    spawner.spawn(tasks::eps::run(eps_handle).expect("EPS task pool is available"));

    pending::<()>().await;
    drop(_reserved);
    unreachable!()
}

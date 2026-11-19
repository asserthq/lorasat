use core::future::pending;
use defmt::info;

use embassy_executor::Spawner;

use crate::{
    board::{self, BoardParts},
    devices::{adcs, radios},
    resources::{
        channels::{self, WorkersHandles},
        shared_i2c::SharedI2cBuses,
    },
    tasks,
};

pub async fn run(spawner: Spawner) -> ! {
    let BoardParts {
        i2c,
        radios,
        flash,
        coils,
    } = board::init();

    let i2c_shared = SharedI2cBuses::init(i2c);

    let radio_devices = radios::init(radios)
        .await
        .expect("SX1262 initialization failed");

    let adcs_devices = adcs::init(&i2c_shared, coils)
        .await
        .expect("MMC5983MA initialization failed");

    info!("CDH, ADCS and EPS initialization complete");

    let radios::RadioDevices {
        platform: platform_radio,
        service: service_radio,
    } = radio_devices;

    let adcs::AdcsDevices {
        magnetometer,
        imu,
        coils,
        sun_sensors,
    } = adcs_devices;

    let (controller_handles, worker_handles) = channels::split();
    let WorkersHandles {
        adcs: adcs_handle,
        eps: eps_handle,
        service: service_handle,
    } = worker_handles;

    // Retain resources whose tasks are not wired yet. Dropping their HAL
    // drivers would deinitialize the corresponding peripherals.
    let _reserved = (
        service_radio,
        service_handle,
        flash,
        i2c_shared,
        imu,
        sun_sensors,
    );

    spawner.spawn(
        tasks::cdh::run(platform_radio, controller_handles).expect("CDH task pool is available"),
    );
    spawner.spawn(
        tasks::adcs::run(magnetometer, coils, adcs_handle).expect("ADCS task pool is available"),
    );
    spawner.spawn(tasks::eps::run(eps_handle).expect("EPS task pool is available"));

    pending::<()>().await;
    drop(_reserved);
    unreachable!()
}

use defmt::info;
use embassy_executor::Spawner;

use crate::{board, devices::radios, tasks};

pub async fn run(_spawner: Spawner) -> ! {
    let board = board::init();
    let radios = radios::init(board.command_radio, board.client_radio)
        .await
        .expect("SX1262 initialization failed");

    info!("both SX1262 radios initialized");

    let radios::Radios {
        platform: command,
        service: _client,
    } = radios;

    tasks::cdh::run(command).await
}

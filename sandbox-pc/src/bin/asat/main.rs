mod mock;
use mock::*;

use sat_core::adcs::{command::AdcsCommand, *};

use std::io;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    let mag = MagMock {};
    let coils = CoilsMock {};
    let delay = DelayMock {};
    let mut adcs = Adcs::new(mag, coils, delay);
    adcs.send_command(AdcsCommand::SetMode(AdcsMode::Detumbling));
    adcs.run().await;
    Ok(())
}

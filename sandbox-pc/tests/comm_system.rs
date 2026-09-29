//! Интеграционный тест CommSystem на std-рантайме (tokio).
//!
//! Подсистема не меняется: embassy-futures — чистые futures,
//! embassy-time/std даёт time driver, defmt-or-log/log пишет в `log`.

use std::time::Duration;

use sandbox_pc::mock::TransportMockTCP;
use sat_core::comm::TransportLayer;
use sat_core::comm::address::Address;
use sat_subsystems::comm::{CommConfig, CommSystem};

const GS: Address = Address(0x01);
const SAT: Address = Address(0x11);
const GS2: Address = Address(0x02);
const SAT2: Address = Address(0x12);

fn spawn_comm() -> tokio::task::JoinHandle<()> {
    let rt = tokio::runtime::Handle::current();
    rt.spawn(async {
        let transport = TransportMockTCP::new(SAT, &[GS]).await.unwrap();
        let config = CommConfig {
            sat_addr: SAT,
            beacon_interval_sec: 1,
        };
        let mut comm = CommSystem::new(transport, config);
        comm.run().await;
    })
}

fn spawn_comm2() -> tokio::task::JoinHandle<()> {
    let rt = tokio::runtime::Handle::current();
    rt.spawn(async {
        let transport = TransportMockTCP::new(SAT2, &[GS2]).await.unwrap();
        let config = CommConfig {
            sat_addr: SAT2,
            beacon_interval_sec: 1,
        };
        let mut comm = CommSystem::new(transport, config);
        comm.run().await;
    })
}

#[tokio::test]
async fn comm_handles_datagram_and_stays_alive() {
    let _ = env_logger::try_init();
    let comm = spawn_comm();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // GS шлёт датаграмму через TransportMockTCP (с tag-байтом)
    let mut gs = TransportMockTCP::new(GS, &[]).await.unwrap();
    gs.send_datagram(SAT, b"PING").await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!comm.is_finished(), "comm task died after datagram");

    comm.abort();
}

#[tokio::test]
async fn comm_accepts_session_from_gs() {
    let _ = env_logger::try_init();
    let comm = spawn_comm2();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // GS открывает TCP-сессию к спутнику через тот же мок-транспорт
    let mut gs = TransportMockTCP::new(GS2, &[]).await.unwrap();
    let _session = gs.connect(SAT2).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!comm.is_finished(), "comm task died after session accept");

    comm.abort();
}

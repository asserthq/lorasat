//! Интеграционный тест CommSystem на std-рантайме (tokio).
//!
//! Подсистема не меняется: embassy-futures — чистые futures,
//! embassy-time/std даёт time driver, defmt-or-log/log пишет в `log`.

use std::time::Duration;

use sandbox_pc::mock::TransportMockTCP;
use sat_core::comm::address::Address;
use sat_core::comm::transport::TransportLayer;
use sat_subsystems::comm::{CommConfig, CommSystem};
use tokio::net::UdpSocket;

const GS: Address = Address(0x01);
const SAT: Address = Address(0x11);

fn spawn_comm() -> tokio::task::JoinHandle<()> {
    let rt = tokio::runtime::Handle::current();
    rt.spawn(async {
        let transport = TransportMockTCP::from_addr(SAT).await.unwrap();
        let config = CommConfig {
            sat_addr: SAT,
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
    // дать задаче подняться и забиндить сокеты
    tokio::time::sleep(Duration::from_millis(100)).await;

    // GS шлёт датаграмму на спутник напрямую через UDP
    let gs = UdpSocket::bind("127.0.0.1:9001").await.unwrap();
    gs.send_to(b"PING", "127.0.0.1:9017").await.unwrap();

    // CommSystem::handle_gs_datagram только логирует; проверяем, что задача
    // пережила приём и не упала
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!comm.is_finished(), "comm task died after datagram");

    comm.abort();
}

#[tokio::test]
async fn comm_accepts_session_from_gs() {
    let _ = env_logger::try_init();
    let comm = spawn_comm();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // GS открывает TCP-сессию к спутнику через тот же мок-транспорт
    let mut gs = TransportMockTCP::from_addr(GS).await.unwrap();
    let _session = gs.connect(SAT).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!comm.is_finished(), "comm task died after session accept");

    comm.abort();
}

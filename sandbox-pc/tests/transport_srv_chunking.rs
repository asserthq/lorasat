//! Интеграционный тест: TransportSrv → LinkImpl → PhyRadioUDP.
//!
//! Проверяет много-чанковую передачу через сессию с реальным
//! чанкованием (MTU=240, ACK bitmap u16, ретрансмит).

use sandbox_pc::mock::PhyRadioUDP;
use sat_core::comm::address::Address;
use sat_core::comm::transport::{TransportEvent, TransportLayer, TransportSession};
use sat_drivers::proto_impl::link::LinkImpl;
use sat_drivers::proto_impl::transport::TransportImpl;

const GS: Address = Address(0x01);
const SAT: Address = Address(0x11);

/// Полный стек: TransportSrv → LinkImpl → PhyRadioUDP.
/// Отправка 500 байт (3 чанка) через сессию, приём, проверка целостности.
#[tokio::test]
async fn session_multi_chunk_500_bytes() {
    let _ = env_logger::try_init();

    // ── Физический уровень (радио-модель на UDP) ──
    let gs_phy = PhyRadioUDP::new(GS, &[SAT]).await.unwrap();
    let sat_phy = PhyRadioUDP::new(SAT, &[GS]).await.unwrap();

    // ── Канальный уровень ──
    let gs_link = LinkImpl::new(GS, gs_phy);
    let sat_link = LinkImpl::new(SAT, sat_phy);

    // ── Транспортный уровень ──
    let mut gs_transport = TransportImpl::new(gs_link);
    let mut sat_transport = TransportImpl::new(sat_link);

    // SAT в фоне принимает входящий Connect
    let sat_session = tokio::spawn(async move {
        let mut buf = [0u8; 512];
        loop {
            match sat_transport.next(&mut buf).await {
                Ok(TransportEvent::Session(s)) => return s,
                Ok(_) => continue, // пропускаем маяки/датаграммы
                Err(e) => panic!("sat next() error: {e:?}"),
            }
        }
    });

    // GS открывает сессию (блокируется до Accept от SAT)
    let mut gs_session = gs_transport.connect(SAT).await.unwrap();
    let mut sat_session = sat_session.await.unwrap();

    // ── Обмен: 500 байт = 3 чанка (240 + 240 + 20) ──
    let payload = [0xABu8; 500];
    let mut sat_buf = [0u8; 1024];

    let (send_result, recv_result) =
        tokio::join!(gs_session.send(&payload), sat_session.recv(&mut sat_buf),);

    send_result.unwrap();
    let received = recv_result.unwrap();
    assert_eq!(received, &payload[..], "received data must match sent");
}

/// Отправка ровно MTU байт (240) — один чанк.
#[tokio::test]
async fn session_single_chunk_mtu_exact() {
    let _ = env_logger::try_init();

    let gs_phy = PhyRadioUDP::new(Address(0x30), &[Address(0x40)])
        .await
        .unwrap();
    let sat_phy = PhyRadioUDP::new(Address(0x40), &[Address(0x30)])
        .await
        .unwrap();

    let mut gs = TransportImpl::new(LinkImpl::new(Address(0x30), gs_phy));
    let mut sat = TransportImpl::new(LinkImpl::new(Address(0x40), sat_phy));

    let sat_sess = tokio::spawn(async move {
        let mut buf = [0u8; 512];
        loop {
            match sat.next(&mut buf).await {
                Ok(TransportEvent::Session(s)) => return s,
                Ok(_) => continue,
                Err(e) => panic!("sat error: {e:?}"),
            }
        }
    });

    let mut gs_sess = gs.connect(Address(0x40)).await.unwrap();
    let mut sat_sess = sat_sess.await.unwrap();

    let payload = [0xCDu8; 240]; // ровно MTU
    let mut sat_buf = [0u8; 512];

    let (send, recv) = tokio::join!(gs_sess.send(&payload), sat_sess.recv(&mut sat_buf));

    send.unwrap();
    assert_eq!(recv.unwrap(), &payload[..]);
}

/// Отправка пустого сообщения — должен вернуть Ok без передачи.
#[tokio::test]
async fn session_empty_send() {
    let _ = env_logger::try_init();

    let gs_phy = PhyRadioUDP::new(Address(0x50), &[Address(0x60)])
        .await
        .unwrap();
    let sat_phy = PhyRadioUDP::new(Address(0x60), &[Address(0x50)])
        .await
        .unwrap();

    let mut gs = TransportImpl::new(LinkImpl::new(Address(0x50), gs_phy));
    let mut sat = TransportImpl::new(LinkImpl::new(Address(0x60), sat_phy));

    let sat_sess = tokio::spawn(async move {
        let mut buf = [0u8; 512];
        loop {
            match sat.next(&mut buf).await {
                Ok(TransportEvent::Session(s)) => return s,
                Ok(_) => continue,
                Err(e) => panic!("sat error: {e:?}"),
            }
        }
    });

    let mut gs_sess = gs.connect(Address(0x60)).await.unwrap();
    let _sat_sess = sat_sess.await.unwrap();

    // Пустой send — должен Ok
    gs_sess.send(&[]).await.unwrap();
}

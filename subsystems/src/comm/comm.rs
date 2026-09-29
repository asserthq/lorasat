use defmt_or_log::*;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Ticker};
use sat_core::comm::{
    Address, TransportEvent, TransportLayer, TransportSession, address::BROADCAST_ADDRESS,
};
use sat_core::entity::Beacon;

use crate::comm::CommConfig;

pub struct CommSystem<T: TransportLayer> {
    transport: T,
    config: CommConfig,
}

impl<T: TransportLayer> CommSystem<T> {
    pub fn new(transport: T, config: CommConfig) -> Self {
        Self { transport, config }
    }

    pub async fn run(&mut self) {
        info!("comm system running");
        let mut beacon_ticker =
            Ticker::every(Duration::from_secs(self.config.beacon_interval_sec as u64));

        let mut buf = [0u8; 255];

        loop {
            let accept_gs = self.transport.next(&mut buf);

            match select(accept_gs, beacon_ticker.next()).await {
                Either::First(e) => self.handle_gs_event(e.unwrap()).await,
                Either::Second(_) => self.send_beacon().await,
            }
        }
    }

    async fn send_beacon(&mut self) {
        info!("sending beacon");
        let beacon = Beacon {
            sat_addr: self.config.sat_addr,
            interval_sec: self.config.beacon_interval_sec,
            timestamp: Instant::now().elapsed().as_millis(),
        };
        let mut buf = [0u8; 255];
        let payload = postcard::to_slice(&beacon, &mut buf).unwrap();
        if self.transport.send_beacon(payload).await.is_err() {
            warn!("beacon send failed");
        }
    }

    async fn handle_gs_event(&mut self, event: TransportEvent<'_, T::Session>) {
        info!("transport event");
        match event {
            TransportEvent::Datagram { from, data } => self.handle_gs_datagram(from, data).await,
            TransportEvent::Session(s) => self.handle_gs_session(s).await,
            TransportEvent::Beacon { payload } => {}
        }
    }

    async fn handle_gs_datagram(&mut self, from: Address, data: &[u8]) {
        info!("recieved datagram from address: {:?}", from);
        if data == b"fetch" {
            info!("fetch command — sending dummy payload to {:?}", from);
            let dummy = generate_dummy_data(2048);
            match self.transport.connect(from).await {
                Ok(mut session) => {
                    if let Err(e) = session.send(&dummy).await {
                        warn!("dummy send failed: {:?}", e);
                    } else {
                        info!("sent {} bytes of dummy data", dummy.len());
                    }
                }
                Err(e) => warn!("connect to {:?} failed: {:?}", from, e),
            }
        }
    }

    async fn handle_gs_session(&mut self, session: T::Session) {
        info!("new session from: {:?}", session.peer_addr());
    }
}

/// Генерирует тестовые данные заданного размера.
/// Паттерн: 0x00, 0x01, 0x02, ..., 0xFF, 0x00, ...
fn generate_dummy_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

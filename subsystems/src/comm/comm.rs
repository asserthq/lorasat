use defmt_or_log::{info, warn};
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Ticker};
use sat_core::comm::address::{Address, BROADCAST_ADDRESS};
use sat_core::comm::transport::{Event, Session};
use sat_core::{comm::transport::TransportLayer, entity::Beacon};

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
        let mut beacon_ticker =
            Ticker::every(Duration::from_secs(self.config.beacon_interval_sec as u64));
        // Локальный, не поле struct: Event держит заём buf, иначе он
        // конфликтует с &mut self в обработчиках (E0499).
        let mut buf = [0u8; 255];

        info!("comm system running");

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
        if self
            .transport
            .send_datagram(BROADCAST_ADDRESS, payload)
            .await
            .is_err()
        {
            warn!("beacon send failed");
        }
    }

    async fn handle_gs_event(&mut self, event: Event<'_, T::Session>) {
        info!("transport event");
        match event {
            Event::Datagram { from, data } => self.handle_gs_datagram(from, data).await,
            Event::Session(s) => self.handle_gs_session(s).await,
        }
    }

    async fn handle_gs_datagram(&mut self, from: Address, _data: &[u8]) {
        info!("recieved datagram from address: {:?}", from);
    }

    async fn handle_gs_session(&mut self, session: T::Session) {
        info!("new session from: {:?}", session.peer_addr());
    }
}

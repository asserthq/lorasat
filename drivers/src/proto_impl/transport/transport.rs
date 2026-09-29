//! Реализация [`TransportLayer`] поверх [`LinkLayer`].
//!
//! # Различение типов трафика
//!
//! Каждый Data-кадр канального уровня начинается с tag-байта:
//!
//! ```text
//! ┌──────┬────────────────────────────┐
//! │ 0x00 │ raw payload (datagram)     │
//! ├──────┼────────────────────────────┤
//! │ 0x01 │ postcard-encoded SrvPdu    │
//! └──────┴────────────────────────────┘
//! ```
//!
//! Маяки идут через `send_beacon`/`RecvFrame::Beacon` — без tag-байта.
//!
//! # Владение канальным уровнем
//!
//! `TransportSrv` хранит `Option<L>`. При `connect()` или входящей сессии
//! `L` перемещается в `SessionImpl`. Пока link отсутствует, остальные
//! методы возвращают `Busy`.

use embassy_time::Instant;
use heapless::Vec;
use sat_core::comm::address::Address;
use sat_core::comm::link::{LinkLayer, RecvFrame};
use sat_core::comm::transport::{TransportEvent, TransportLayer, TransportSession};

use super::pdu::{MAX_CHUNKS, MTU, SrvPdu};

const TAG_DATAGRAM: u8 = 0x00;
const TAG_SRV_PDU: u8 = 0x01;
const RETRANSMIT_MS: u64 = 2000;

// ═══════════════════════════════════════════════════════════════════════════
//  TransportSrv
// ═══════════════════════════════════════════════════════════════════════════

pub struct TransportImpl<L: LinkLayer> {
    link: Option<L>,
    encode_buf: [u8; 300],
}

impl<L: LinkLayer> TransportImpl<L> {
    pub fn new(link: L) -> Self {
        Self {
            link: Some(link),
            encode_buf: [0u8; 300],
        }
    }
}

impl<L: LinkLayer> TransportLayer for TransportImpl<L> {
    type Error = TransportError;
    type Session = SessionImpl<L>;

    async fn send_beacon(&mut self, beacon: &[u8]) -> Result<(), Self::Error> {
        match &mut self.link {
            Some(link) => link
                .send_beacon(beacon)
                .await
                .map_err(|_| TransportError::Link),
            None => Err(TransportError::Busy),
        }
    }

    async fn send_datagram(&mut self, dst: Address, data: &[u8]) -> Result<(), Self::Error> {
        if data.len() > MTU {
            return Err(TransportError::PayloadTooLarge);
        }
        self.encode_buf[0] = TAG_DATAGRAM;
        let total = 1 + data.len();
        self.encode_buf[1..total].copy_from_slice(data);
        match &mut self.link {
            Some(link) => link
                .send_frame(dst, &self.encode_buf[..total])
                .await
                .map_err(|_| TransportError::Link),
            None => Err(TransportError::Busy),
        }
    }

    async fn connect<'a>(&mut self, dst: Address) -> Result<Self::Session, Self::Error> {
        // Шаг 1: отправить Connect
        self.encode_buf[0] = TAG_SRV_PDU;
        let n = {
            let wire = postcard::to_slice(&SrvPdu::Connect, &mut self.encode_buf[1..])
                .map_err(|_| TransportError::Encode)?;
            wire.len()
        };
        {
            let link = match &mut self.link {
                Some(link) => link,
                None => return Err(TransportError::Busy),
            };
            link.send_frame(dst, &self.encode_buf[..1 + n])
                .await
                .map_err(|_| TransportError::Link)?;
        }

        // Шаг 2: ждать Accept
        let mut buf = [0u8; 300];
        loop {
            let recv = match &mut self.link {
                Some(link) => link
                    .recv_frame(&mut buf)
                    .await
                    .map_err(|_| TransportError::Link)?,
                None => return Err(TransportError::Busy),
            };
            match recv {
                RecvFrame::Data { src: _, payload } => {
                    if payload.len() > 1 && payload[0] == TAG_SRV_PDU {
                        if let Ok(SrvPdu::Accept) = postcard::from_bytes(&payload[1..]) {
                            break;
                        }
                    }
                }
                RecvFrame::Beacon { .. } => {}
            }
        }

        // Шаг 3: забрать link в сессию
        let link = self.link.take().ok_or(TransportError::Busy)?;
        Ok(SessionImpl::new(link, dst))
    }

    async fn next<'a>(
        &mut self,
        buf: &'a mut [u8],
    ) -> Result<TransportEvent<'a, Self::Session>, Self::Error> {
        // Локальный буфер для приёма — обходим проблему повторного borrow buf в цикле.
        let mut local = [0u8; 300];
        loop {
            let recv = match &mut self.link {
                Some(link) => link
                    .recv_frame(&mut local)
                    .await
                    .map_err(|_| TransportError::Link)?,
                None => return Err(TransportError::Busy),
            };
            match recv {
                RecvFrame::Beacon { payload } => {
                    let len = payload.len().min(buf.len());
                    buf[..len].copy_from_slice(&payload[..len]);
                    return Ok(TransportEvent::Beacon {
                        payload: &buf[..len],
                    });
                }
                RecvFrame::Data { src, payload } => {
                    if payload.is_empty() {
                        continue;
                    }
                    match payload[0] {
                        TAG_DATAGRAM => {
                            let data = &payload[1..];
                            let len = data.len().min(buf.len());
                            buf[..len].copy_from_slice(&data[..len]);
                            return Ok(TransportEvent::Datagram {
                                from: src,
                                data: &buf[..len],
                            });
                        }
                        TAG_SRV_PDU => {
                            if let Ok(pdu) = postcard::from_bytes::<SrvPdu>(&payload[1..]) {
                                match pdu {
                                    SrvPdu::Connect => {
                                        // Ответить Accept
                                        self.encode_buf[0] = TAG_SRV_PDU;
                                        let n = {
                                            let wire = postcard::to_slice(
                                                &SrvPdu::Accept,
                                                &mut self.encode_buf[1..],
                                            )
                                            .map_err(|_| TransportError::Encode)?;
                                            wire.len()
                                        };
                                        {
                                            let link = match &mut self.link {
                                                Some(l) => l,
                                                None => return Err(TransportError::Busy),
                                            };
                                            link.send_frame(src, &self.encode_buf[..1 + n])
                                                .await
                                                .map_err(|_| TransportError::Link)?;
                                        }
                                        let link = self.link.take().ok_or(TransportError::Busy)?;
                                        return Ok(TransportEvent::Session(SessionImpl::new(
                                            link, src,
                                        )));
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  SessionImpl — сессия надёжной передачи
// ═══════════════════════════════════════════════════════════════════════════

pub struct SessionImpl<L: LinkLayer> {
    link: L,
    peer: Address,
    encode_buf: [u8; 300],

    // --- Передатчик ---
    send_seq: u32,
    tx_chunks: [[u8; MTU]; MAX_CHUNKS],
    tx_lens: [u8; MAX_CHUNKS],
    tx_base_seq: u32,
    tx_num_chunks: u8,
    tx_acked: u32,
    tx_last_retransmit: Instant,

    // --- Приёмник ---
    recv_next: u32,
    recv_bitmap: u16,
    recv_buf: [u8; MAX_CHUNKS * MTU],
    recv_len: usize,
    recv_active: bool,
}

impl<L: LinkLayer> SessionImpl<L> {
    pub fn new(link: L, peer: Address) -> Self {
        Self {
            link,
            peer,
            encode_buf: [0u8; 300],
            send_seq: 0,
            tx_chunks: [[0u8; MTU]; MAX_CHUNKS],
            tx_lens: [0u8; MAX_CHUNKS],
            tx_base_seq: 0,
            tx_num_chunks: 0,
            tx_acked: 0,
            tx_last_retransmit: Instant::now(),
            recv_next: 0,
            recv_bitmap: 0,
            recv_buf: [0u8; MAX_CHUNKS * MTU],
            recv_len: 0,
            recv_active: false,
        }
    }

    async fn send_pdu(&mut self, pdu: &SrvPdu) -> Result<(), TransportError> {
        self.encode_buf[0] = TAG_SRV_PDU;
        let n = {
            let wire = postcard::to_slice(pdu, &mut self.encode_buf[1..])
                .map_err(|_| TransportError::Encode)?;
            wire.len()
        };
        let total = 1 + n;
        self.link
            .send_frame(self.peer, &self.encode_buf[..total])
            .await
            .map_err(|_| TransportError::Link)
    }

    /// Принять SrvPdu (Data или Ack). Маяки/датаграммы пропускает.
    async fn recv_pdu(&mut self, buf: &mut [u8]) -> Result<SrvPdu, TransportError> {
        loop {
            let recv = self
                .link
                .recv_frame(buf)
                .await
                .map_err(|_| TransportError::Link)?;
            match recv {
                RecvFrame::Data { src: _, payload } => {
                    if payload.len() > 1 && payload[0] == TAG_SRV_PDU {
                        if let Ok(pdu) = postcard::from_bytes::<SrvPdu>(&payload[1..]) {
                            if matches!(pdu, SrvPdu::Connect | SrvPdu::Accept) {
                                continue;
                            }
                            return Ok(pdu);
                        }
                    }
                }
                RecvFrame::Beacon { .. } => {}
            }
        }
    }

    fn process_ack(&mut self, cumulative_seq: u32, bitmap: u16) {
        for i in 0..self.tx_num_chunks as u32 {
            let seq = self.tx_base_seq + i;
            if seq <= cumulative_seq {
                self.tx_acked |= 1 << i;
            } else if seq <= cumulative_seq + 16 {
                let bit = seq - cumulative_seq - 1;
                if bit < 16 && ((bitmap >> bit) & 1) == 1 {
                    self.tx_acked |= 1 << i;
                }
            }
        }
    }

    fn handle_data(&mut self, seq: u32, first: bool, payload: &[u8]) {
        if first {
            self.recv_active = true;
            self.recv_len = 0;
            self.recv_next = seq;
        }
        if !self.recv_active {
            return;
        }
        if seq == self.recv_next {
            let end = self.recv_len + payload.len();
            if end <= self.recv_buf.len() {
                self.recv_buf[self.recv_len..end].copy_from_slice(payload);
                self.recv_len = end;
            }
            self.recv_next = seq + 1;
            while (self.recv_bitmap & 1) == 1 {
                self.recv_next += 1;
                self.recv_bitmap >>= 1;
            }
        } else if seq > self.recv_next && seq < self.recv_next + 16 {
            let bit = seq - self.recv_next - 1;
            self.recv_bitmap |= 1 << bit;
        }
    }

    fn build_ack(&self) -> SrvPdu {
        SrvPdu::Ack {
            cumulative_seq: self.recv_next.wrapping_sub(1),
            bitmap: self.recv_bitmap,
        }
    }

    async fn retransmit_unacked(&mut self) -> Result<(), TransportError> {
        for i in 0..self.tx_num_chunks {
            if (self.tx_acked >> i) & 1 == 0 {
                let seq = self.tx_base_seq + i as u32;
                let first = i == 0;
                let last = i == self.tx_num_chunks - 1;
                let len = self.tx_lens[i as usize] as usize;
                let payload: Vec<u8, MTU> = Vec::from_slice(&self.tx_chunks[i as usize][..len])
                    .map_err(|_| TransportError::Encode)?;
                self.send_pdu(&SrvPdu::Data {
                    seq,
                    first,
                    last,
                    payload,
                })
                .await?;
            }
        }
        self.tx_last_retransmit = Instant::now();
        Ok(())
    }
}

impl<L: LinkLayer> TransportSession for SessionImpl<L> {
    type Error = TransportError;

    fn peer_addr(&self) -> Address {
        self.peer
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        if data.is_empty() {
            return Ok(());
        }
        if data.len() > MAX_CHUNKS * MTU {
            return Err(TransportError::PayloadTooLarge);
        }

        let num_chunks = ((data.len() + MTU - 1) / MTU) as u8;
        let base_seq = self.send_seq;
        self.send_seq = self.send_seq.wrapping_add(num_chunks as u32);

        for i in 0..num_chunks as usize {
            let start = i * MTU;
            let end = (start + MTU).min(data.len());
            let len = end - start;
            self.tx_chunks[i][..len].copy_from_slice(&data[start..end]);
            self.tx_lens[i] = len as u8;
        }
        self.tx_base_seq = base_seq;
        self.tx_num_chunks = num_chunks;
        self.tx_acked = 0;
        self.tx_last_retransmit = Instant::now();

        for i in 0..num_chunks {
            let seq = base_seq + i as u32;
            let first = i == 0;
            let last = i == num_chunks - 1;
            let len = self.tx_lens[i as usize] as usize;
            let payload: Vec<u8, MTU> = Vec::from_slice(&self.tx_chunks[i as usize][..len])
                .map_err(|_| TransportError::Encode)?;
            self.send_pdu(&SrvPdu::Data {
                seq,
                first,
                last,
                payload,
            })
            .await?;
        }

        let all_mask = if num_chunks == 32 {
            u32::MAX
        } else {
            (1u32 << num_chunks) - 1
        };

        let mut buf = [0u8; 300];
        loop {
            if self.tx_acked == all_mask {
                return Ok(());
            }

            let pdu = self.recv_pdu(&mut buf).await?;
            match pdu {
                SrvPdu::Ack {
                    cumulative_seq,
                    bitmap,
                } => {
                    self.process_ack(cumulative_seq, bitmap);
                }
                SrvPdu::Data {
                    seq,
                    first,
                    last: _last,
                    payload,
                } => {
                    self.handle_data(seq, first, &payload);
                    let ack = self.build_ack();
                    self.send_pdu(&ack).await?;
                }
                _ => {}
            }

            if self.tx_acked != all_mask
                && Instant::now()
                    .duration_since(self.tx_last_retransmit)
                    .as_millis()
                    >= RETRANSMIT_MS as u64
            {
                self.retransmit_unacked().await?;
            }
        }
    }

    async fn recv<'a>(&'a mut self, buf: &'a mut [u8]) -> Result<&'a [u8], Self::Error> {
        let mut decode_buf = [0u8; 300];

        loop {
            let pdu = self.recv_pdu(&mut decode_buf).await?;
            match pdu {
                SrvPdu::Data {
                    seq,
                    first,
                    last,
                    payload,
                } => {
                    self.handle_data(seq, first, &payload);
                    let ack = self.build_ack();
                    self.send_pdu(&ack).await?;

                    if last && self.recv_len > 0 {
                        let len = self.recv_len.min(buf.len());
                        buf[..len].copy_from_slice(&self.recv_buf[..len]);
                        self.recv_active = false;
                        return Ok(&buf[..len]);
                    }
                }
                SrvPdu::Ack { .. } => {}
                _ => {}
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Ошибки
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, PartialEq, Eq)]
pub enum TransportError {
    Link,
    Timeout,
    Encode,
    Decode,
    PayloadTooLarge,
    /// Link передан в сессию — операции транспорта недоступны.
    Busy,
}

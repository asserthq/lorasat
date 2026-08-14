use heapless::Vec;

use crate::layer::data_link::{DataLinkFrame, DataLinkLayer, FrameKind};
use crate::layer::transport::{TransportLayer, TransportMessage};

use super::frp_error::FrpError;
use super::frp_message::{
    FrpAck, FrpCheck, FrpData, FrpStart, TYPE_ACK, TYPE_CHECK, TYPE_DATA, TYPE_START,
};
use super::frp_session::{ReceiverSession, SenderSession};

// ── FrpPacket classifier (internal) ──

enum FrpPacket {
    Start(FrpStart),
    Data(FrpData),
    Ack(FrpAck),
    Check(FrpCheck),
}

// ── FrpTransport ──

pub struct FrpTransport<L: DataLinkLayer> {
    link: L,
    src_addr: u32,
    chunk_size: u16,
    next_session_id: u16,
    session_src: u32,
}

impl<L: DataLinkLayer> FrpTransport<L> {
    pub fn new(link: L, src_addr: u32, chunk_size: u16) -> Self {
        Self {
            link,
            src_addr,
            chunk_size,
            next_session_id: 1,
            session_src: 0,
        }
    }

    fn next_id(&mut self) -> u16 {
        let id = self.next_session_id;
        self.next_session_id = self.next_session_id.wrapping_add(1);
        id
    }

    /// Classify raw bytes from a received Data frame into FRP packet type.
    fn classify(data: &[u8]) -> Result<FrpPacket, FrpError> {
        if data.is_empty() {
            return Err(FrpError::InvalidControlByte);
        }
        let msg_type = data[0] & 0x0F;
        match msg_type {
            TYPE_START => Ok(FrpPacket::Start(FrpStart::try_decode(data)?)),
            TYPE_DATA => Ok(FrpPacket::Data(FrpData::try_decode(data)?)),
            TYPE_ACK => Ok(FrpPacket::Ack(FrpAck::try_decode(data)?)),
            TYPE_CHECK => Ok(FrpPacket::Check(FrpCheck::try_decode(data)?)),
            _ => Err(FrpError::InvalidControlByte),
        }
    }

    /// Wrap serialized FRP bytes into a `DataLinkFrame`.
    fn wrap_frp_bytes(&self, bytes: &[u8], dest_addr: u32) -> Result<DataLinkFrame, FrpError> {
        let data_vec = Vec::from_slice(bytes).map_err(|_| FrpError::Encode)?;
        Ok(DataLinkFrame {
            kind: FrameKind::Transport,
            src_addr: self.src_addr,
            dest_addr,
            flags: 0,
            data: data_vec,
        })
    }

    /// Receive frames until a `FrpCheck` for the expected session arrives.
    async fn recv_check(
        &mut self,
        expected_session: u16,
        buf: &mut [u8],
    ) -> Result<FrpCheck, FrpError> {
        loop {
            let frame = self
                .link
                .try_recv_frame(buf)
                .await
                .map_err(|_| FrpError::Decode)?;
            if frame.kind == FrameKind::Transport {
                if let Ok(FrpPacket::Check(check)) = Self::classify(&frame.data) {
                    if check.session_id == expected_session {
                        return Ok(check);
                    }
                }
            }
        }
    }

    /// Receive frames until a `FrpStart` arrives.
    async fn recv_start(&mut self, buf: &mut [u8]) -> Result<FrpStart, FrpError> {
        loop {
            let frame = self
                .link
                .try_recv_frame(buf)
                .await
                .map_err(|_| FrpError::Decode)?;
            if frame.kind == FrameKind::Transport {
                if let Ok(FrpPacket::Start(start)) = Self::classify(&frame.data) {
                    self.session_src = frame.src_addr;
                    return Ok(start);
                }
            }
        }
    }

    /// Receive frames until a `FrpData` for the expected session arrives.
    async fn recv_data(
        &mut self,
        expected_session: u16,
        buf: &mut [u8],
    ) -> Result<FrpData, FrpError> {
        loop {
            let frame = self
                .link
                .try_recv_frame(buf)
                .await
                .map_err(|_| FrpError::Decode)?;
            if frame.kind == FrameKind::Transport {
                if let Ok(FrpPacket::Data(frp_data)) = Self::classify(&frame.data) {
                    if frp_data.session_id == expected_session {
                        return Ok(frp_data);
                    }
                }
            }
        }
    }

    /// Simple wrapping-add checksum over all bytes.
    fn compute_checksum(data: &[u8]) -> u32 {
        let mut sum: u32 = 0;
        for &b in data {
            sum = sum.wrapping_add(b as u32);
        }
        sum
    }
}

impl<L: DataLinkLayer> core::fmt::Debug for FrpTransport<L> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FrpTransport")
            .field("src_addr", &self.src_addr)
            .field("chunk_size", &self.chunk_size)
            .field("next_session_id", &self.next_session_id)
            .finish_non_exhaustive()
    }
}

// ── TransportLayer impl ──

impl<L: DataLinkLayer> TransportLayer for FrpTransport<L> {
    type Error = FrpError;

    async fn try_send_message(&mut self, msg: TransportMessage) -> Result<(), Self::Error> {
        let dest_addr = msg.addr;
        let data = &msg.payload;
        let session_id = self.next_id();
        let checksum = Self::compute_checksum(data);
        let mut session = SenderSession::new(data, self.chunk_size, session_id, checksum)?;

        // Scratch buffers: encode_buf for serializing FRP packets,
        // rx_buf for receiving frames from the link.
        let mut encode_buf = [0u8; 256];
        let mut rx_buf = [0u8; 256];

        // Phase 1 — send FrpStart + all chunks.
        loop {
            let packet = session.next_packet(&mut encode_buf)?;
            let Some(packet) = packet else { break };
            let wrapped = self.wrap_frp_bytes(packet, dest_addr)?;
            // packet borrow ends here — encode_buf is free again.
            self.link
                .try_send_frame(wrapped)
                .await
                .map_err(|_| FrpError::Encode)?;
        }

        // Phase 2 — receive FrpCheck, retransmit missing chunks, repeat.
        loop {
            let check = self.recv_check(session_id, &mut rx_buf).await?;

            if SenderSession::is_session_success(check.flags) {
                break;
            }

            session.process_check(&check)?;

            loop {
                let packet = session.next_packet(&mut encode_buf)?;
                let Some(packet) = packet else { break };
                let wrapped = self.wrap_frp_bytes(packet, dest_addr)?;
                self.link
                    .try_send_frame(wrapped)
                    .await
                    .map_err(|_| FrpError::Encode)?;
            }
        }

        Ok(())
    }

    async fn try_recv_message<'a>(
        &'a mut self,
        _buf: &'a mut [u8],
    ) -> Result<TransportMessage, Self::Error> {
        let mut rx_buf = [0u8; 256];
        let mut encode_buf = [0u8; 256];

        // 1. Wait for FrpStart.
        let start = self.recv_start(&mut rx_buf).await?;

        // 2. Create and initialise receiver session.
        let mut session = ReceiverSession::new();
        session.handle_start(&start)?;

        let session_id = start.session_id;

        // 3. Receive chunks until all arrived, ack each.
        while session.assembled_message().is_none() {
            let frp_data = self.recv_data(session_id, &mut rx_buf).await?;
            session.handle_data(&frp_data)?;

            let ack = FrpAck {
                session_id: frp_data.session_id,
                chunk_index: frp_data.chunk_index,
                status: 0x00,
            };
            let ack_slice = ack.try_encode(&mut encode_buf)?;
            let wrapped = self.wrap_frp_bytes(ack_slice, self.session_src)?;
            self.link
                .try_send_frame(wrapped)
                .await
                .map_err(|_| FrpError::Encode)?;
        }

        // 4. Send final FrpCheck (all chunks received).
        let check = session.generate_check()?;
        let check_slice = check.try_encode(&mut encode_buf)?;
        let wrapped = self.wrap_frp_bytes(check_slice, self.session_src)?;
        self.link
            .try_send_frame(wrapped)
            .await
            .map_err(|_| FrpError::Encode)?;

        // 5. Assemble into TransportMessage.
        let msg = session
            .assembled_message()
            .ok_or(FrpError::SessionNotInitialized)?;
        let payload = Vec::from_slice(msg).map_err(|_| FrpError::Encode)?;
        Ok(TransportMessage {
            addr: self.session_src,
            payload,
        })
    }
}

// ── Unit tests ──

#[cfg(test)]
mod tests {
    use super::super::frp_message::{
        FrpAck, FrpCheck, FrpData, FrpStart, MAX_BITMAP_BYTES, MAX_CHUNK_PAYLOAD, MAX_CHUNKS,
    };
    use super::super::frp_session::{ReceiverSession, SenderSession};

    fn checksum(data: &[u8]) -> u32 {
        let mut sum: u32 = 0;
        for &b in data {
            sum = sum.wrapping_add(b as u32);
        }
        sum
    }

    /// Collect every packet a `SenderSession` produces into a `Vec<Vec<u8>>`.
    fn drain_sender(session: &mut SenderSession) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            let packet = session.next_packet(&mut buf).unwrap();
            let Some(slice) = packet else { break };
            packets.push(slice.to_vec());
        }
        packets
    }

    // ── Serialisation round-trips ──

    #[test]
    fn frp_start_roundtrip() {
        let start = FrpStart {
            session_id: 42,
            total_chunks: 10,
            chunk_size: 240,
            message_length: 2000,
            checksum: 0xDEAD_BEEF,
        };
        let mut buf = [0u8; 64];
        let encoded = start.try_encode(&mut buf).unwrap();
        let decoded = FrpStart::try_decode(encoded).unwrap();
        assert_eq!(decoded, start);
    }

    #[test]
    fn frp_data_roundtrip() {
        let payload: heapless::Vec<u8, MAX_CHUNK_PAYLOAD> =
            heapless::Vec::from_slice(&[0xAA; 128]).unwrap();
        let data = FrpData {
            session_id: 7,
            chunk_index: 3,
            flags: 0x00,
            payload,
        };
        let mut buf = [0u8; 256];
        let encoded = data.try_encode(&mut buf).unwrap();
        let decoded = FrpData::try_decode(encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn frp_ack_roundtrip() {
        let ack = FrpAck {
            session_id: 5,
            chunk_index: 2,
            status: 0x01,
        };
        let mut buf = [0u8; 32];
        let encoded = ack.try_encode(&mut buf).unwrap();
        let decoded = FrpAck::try_decode(encoded).unwrap();
        assert_eq!(decoded, ack);
    }

    #[test]
    fn frp_check_roundtrip() {
        let mut bitmap = heapless::Vec::<u8, MAX_BITMAP_BYTES>::new();
        bitmap.resize(4, 0xFF).unwrap();
        let check = FrpCheck {
            session_id: 1,
            total_chunks: 30,
            flags: 0x01,
            ack_bitmap: bitmap,
        };
        let mut buf = [0u8; 64];
        let encoded = check.try_encode(&mut buf).unwrap();
        let decoded = FrpCheck::try_decode(encoded).unwrap();
        assert_eq!(decoded, check);
    }

    #[test]
    fn frp_start_bad_control_byte() {
        let buf = [0xFFu8; 64];
        // Not a valid control byte
        assert!(FrpStart::try_decode(&buf).is_err());
    }

    // ── Session: happy path ──

    #[test]
    fn session_full_happy_path() {
        let message = b"Hello, FRP! This is a test message for chunked file transfer.";
        let chunk_size: u16 = 16;
        let session_id: u16 = 1;
        let csum = checksum(message);

        // Sender
        let mut sender = SenderSession::new(message, chunk_size, session_id, csum).unwrap();
        let packets = drain_sender(&mut sender);
        assert!(sender.is_done());

        // First packet must be FrpStart
        let start = FrpStart::try_decode(&packets[0]).unwrap();
        assert_eq!(start.session_id, session_id);
        assert_eq!(start.total_chunks, 4); // ceil(69 / 16) = 5? wait let me calculate

        // Receiver
        let mut receiver = ReceiverSession::new();
        receiver.handle_start(&start).unwrap();

        // Feed all data packets
        for packet in &packets[1..] {
            let Ok(data) = FrpData::try_decode(packet) else {
                continue;
            };
            receiver.handle_data(&data).unwrap();
        }

        // Check
        let check = receiver.generate_check().unwrap();
        assert_eq!(check.flags, 0x01, "all chunks should be received");

        // Assembled message matches
        let assembled = receiver.assembled_message().unwrap();
        assert_eq!(assembled, message);
    }

    // ── Session: missing chunks + retransmit ──

    #[test]
    fn session_missing_chunks_retransmit() {
        let message = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"; // 36 bytes
        let chunk_size: u16 = 8; // ceil(36/8) = 5 chunks
        let session_id: u16 = 2;
        let csum = checksum(message);

        let mut sender = SenderSession::new(message, chunk_size, session_id, csum).unwrap();
        let packets = drain_sender(&mut sender);

        let start = FrpStart::try_decode(&packets[0]).unwrap();
        assert_eq!(start.total_chunks, 5);

        let mut receiver = ReceiverSession::new();
        receiver.handle_start(&start).unwrap();

        // Feed only chunks 0, 2, 4 — skip 1 and 3
        for packet in &packets[1..] {
            let Ok(data) = FrpData::try_decode(packet) else {
                continue;
            };
            if data.chunk_index == 1 || data.chunk_index == 3 {
                continue; // simulate loss
            }
            receiver.handle_data(&data).unwrap();
        }

        // Check: not all received
        let check = receiver.generate_check().unwrap();
        assert_ne!(check.flags & 0x01, 0x01);

        // Sender processes check
        sender.process_check(&check).unwrap();
        assert!(!sender.is_done());

        // Drain retransmit queue
        let retrans = drain_sender(&mut sender);
        for packet in &retrans {
            let Ok(data) = FrpData::try_decode(packet) else {
                continue;
            };
            receiver.handle_data(&data).unwrap();
        }
        assert!(sender.is_done());

        // Now all received
        let check2 = receiver.generate_check().unwrap();
        assert_eq!(check2.flags, 0x01);
        assert_eq!(receiver.assembled_message().unwrap(), message);
    }

    // ── Session: errors ──

    #[test]
    fn receiver_rejects_wrong_session_id() {
        let mut receiver = ReceiverSession::new();
        let start = FrpStart {
            session_id: 10,
            total_chunks: 3,
            chunk_size: 16,
            message_length: 40,
            checksum: 0,
        };
        receiver.handle_start(&start).unwrap();

        let payload = heapless::Vec::<u8, MAX_CHUNK_PAYLOAD>::from_slice(b"AAAA").unwrap();
        let bad_data = FrpData {
            session_id: 99, // wrong
            chunk_index: 0,
            flags: 0,
            payload,
        };
        assert_eq!(
            receiver.handle_data(&bad_data).unwrap_err(),
            super::super::frp_error::FrpError::InvalidSessionId
        );
    }

    #[test]
    fn receiver_rejects_chunk_index_oob() {
        let mut receiver = ReceiverSession::new();
        let start = FrpStart {
            session_id: 1,
            total_chunks: 3,
            chunk_size: 8,
            message_length: 20,
            checksum: 0,
        };
        receiver.handle_start(&start).unwrap();

        let payload = heapless::Vec::<u8, MAX_CHUNK_PAYLOAD>::from_slice(b"BBBB").unwrap();
        let bad_data = FrpData {
            session_id: 1,
            chunk_index: 5, // out of bounds
            flags: 0,
            payload,
        };
        assert_eq!(
            receiver.handle_data(&bad_data).unwrap_err(),
            super::super::frp_error::FrpError::ChunkIndexOutOfBounds
        );
    }

    #[test]
    fn session_not_initialized() {
        let payload = heapless::Vec::<u8, MAX_CHUNK_PAYLOAD>::from_slice(b"CCCC").unwrap();
        let data = FrpData {
            session_id: 1,
            chunk_index: 0,
            flags: 0,
            payload,
        };
        let mut receiver = ReceiverSession::new();
        assert_eq!(
            receiver.handle_data(&data).unwrap_err(),
            super::super::frp_error::FrpError::SessionNotInitialized
        );
    }

    #[test]
    fn sender_rejects_message_too_large() {
        let big = [0xAAu8; 5000]; // exceeds MAX_MESSAGE (4096)
        assert!(SenderSession::new(&big, 16, 1, 0).is_err());
    }

    #[test]
    fn sender_empty_message() {
        let empty: [u8; 0] = [];
        let mut sender = SenderSession::new(&empty, 16, 1, 0).unwrap();
        // total_chunks should be 0, only start is sent
        let mut buf = [0u8; 32];
        let packet = sender.next_packet(&mut buf).unwrap().unwrap();
        let start = FrpStart::try_decode(packet).unwrap();
        assert_eq!(start.total_chunks, 0);
        assert_eq!(start.message_length, 0);
        // no more packets
        assert!(sender.next_packet(&mut buf).unwrap().is_none());
    }

    #[test]
    fn is_session_success_flag_logic() {
        assert!(SenderSession::is_session_success(0x01));
        assert!(!SenderSession::is_session_success(0x00));
        assert!(!SenderSession::is_session_success(0x03)); // error flag set
    }

    // ── Protocol constants ──

    #[test]
    fn max_chunk_payload_fits_in_data_frame() {
        // Max FRP data packet = 1 (control) + postcard(FrpData with 240 payload)
        // FrpData postcard: 2(u16) + 2(u16) + 1(u8) + (1 + 240)(Vec) = 246
        // Total = 247. Must be ≤ 256 (Data.data capacity).
        assert!(MAX_CHUNK_PAYLOAD <= 240);
        // Quick sanity: serialise a max-size FrpData
        let payload =
            heapless::Vec::<u8, MAX_CHUNK_PAYLOAD>::from_slice(&[0xFF; MAX_CHUNK_PAYLOAD]).unwrap();
        let data = FrpData {
            session_id: 1,
            chunk_index: 0,
            flags: 0,
            payload,
        };
        let mut buf = [0u8; 256];
        let encoded = data.try_encode(&mut buf).unwrap();
        assert!(
            encoded.len() <= 256,
            "encoded FrpData must fit in Data.data capacity"
        );
    }

    #[test]
    fn max_chunks_bitmap_fits() {
        assert!(MAX_CHUNKS <= 256);
        assert_eq!(MAX_BITMAP_BYTES, 32); // 256 / 8 = 32
    }
}

use super::frp_error::FrpError;
use super::frp_message::*;
use heapless::Vec;

/// Maximum assembled message size in bytes.
const MAX_MESSAGE: usize = 4096;

pub struct ReceiverSession {
    session_id: u16,
    /// Pre-allocated buffer for the assembled message.
    buffer: [u8; MAX_MESSAGE],
    /// Logical length of data currently in `buffer`.
    buffer_len: usize,
    /// Bitmap of received chunks: bit i = chunk i received.
    bitmap: [u8; MAX_BITMAP_BYTES],
    total_chunks: u16,
    chunk_size: u16,
    expected_checksum: u32,
    is_initialized: bool,
}

impl ReceiverSession {
    pub fn new() -> Self {
        Self {
            session_id: 0,
            buffer: [0u8; MAX_MESSAGE],
            buffer_len: 0,
            bitmap: [0u8; MAX_BITMAP_BYTES],
            total_chunks: 0,
            chunk_size: 0,
            expected_checksum: 0,
            is_initialized: false,
        }
    }

    pub fn handle_start(&mut self, msg: &FrpStart) -> Result<(), FrpError> {
        let msg_len = msg.message_length as usize;
        if msg_len > MAX_MESSAGE {
            return Err(FrpError::BufferTooShort);
        }
        let bitmap_bytes = (msg.total_chunks as usize + 7) / 8;
        if bitmap_bytes > MAX_BITMAP_BYTES {
            return Err(FrpError::BufferTooShort);
        }

        self.session_id = msg.session_id;
        self.total_chunks = msg.total_chunks;
        self.chunk_size = msg.chunk_size;
        self.expected_checksum = msg.checksum;
        self.buffer_len = msg_len;
        // Clear bitmap for the used region (rest stays zero from new/previous clear).
        self.bitmap[..bitmap_bytes].fill(0);
        self.is_initialized = true;

        Ok(())
    }

    pub fn handle_data(&mut self, msg: &FrpData) -> Result<(), FrpError> {
        if !self.is_initialized {
            return Err(FrpError::SessionNotInitialized);
        }
        if msg.session_id != self.session_id {
            return Err(FrpError::InvalidSessionId);
        }
        if msg.chunk_index >= self.total_chunks {
            return Err(FrpError::ChunkIndexOutOfBounds);
        }

        let offset = msg.chunk_index as usize * self.chunk_size as usize;
        let end = offset + msg.payload.len();

        if end > self.buffer_len {
            return Err(FrpError::BufferTooShort);
        }

        self.buffer[offset..end].copy_from_slice(&msg.payload);

        let byte_idx = msg.chunk_index as usize / 8;
        let bit_idx = msg.chunk_index as usize % 8;
        self.bitmap[byte_idx] |= 1 << bit_idx;

        Ok(())
    }

    pub fn generate_check(&self) -> Result<FrpCheck, FrpError> {
        if !self.is_initialized {
            return Err(FrpError::SessionNotInitialized);
        }

        let bitmap_bytes = (self.total_chunks as usize + 7) / 8;
        let mut ack_bitmap = Vec::<u8, MAX_BITMAP_BYTES>::new();
        ack_bitmap
            .extend_from_slice(&self.bitmap[..bitmap_bytes])
            .map_err(|_| FrpError::BufferTooShort)?;

        let all_received = self.is_bitmap_fully_set();
        let flags = if all_received { 0x01 } else { 0x00 };

        Ok(FrpCheck {
            session_id: self.session_id,
            total_chunks: self.total_chunks,
            flags,
            ack_bitmap,
        })
    }

    fn is_bitmap_fully_set(&self) -> bool {
        for i in 0..self.total_chunks as usize {
            let byte_idx = i / 8;
            let bit_idx = i % 8;
            if (self.bitmap[byte_idx] & (1 << bit_idx)) == 0 {
                return false;
            }
        }
        true
    }

    /// Return assembled message if all chunks received.
    /// Returns a reference — no allocation.
    pub fn assembled_message(&self) -> Option<&[u8]> {
        if self.is_initialized && self.is_bitmap_fully_set() {
            Some(&self.buffer[..self.buffer_len])
        } else {
            None
        }
    }
}

/// Sender session: chunks a message and manages retransmissions.
///
/// Stores message as `heapless::Vec<u8, MAX_MESSAGE>` — no per-chunk
/// allocations. Chunk payloads are sliced from the stored message on demand.
/// `tx_queue` holds chunk indices (u16), not serialized packets.
pub struct SenderSession {
    session_id: u16,
    total_chunks: u16,
    chunk_size: u16,
    checksum: u32,
    /// Copy of the original message (for retransmits).
    message: Vec<u8, MAX_MESSAGE>,
    /// Queue of chunk indices to transmit / re-transmit.
    tx_queue: Vec<u16, MAX_CHUNKS>,
    /// Current read position in tx_queue.
    tx_cursor: usize,
    /// Has FrpStart been sent?
    start_sent: bool,
}

impl SenderSession {
    pub fn new(
        message: &[u8],
        chunk_size: u16,
        session_id: u16,
        checksum: u32,
    ) -> Result<Self, FrpError> {
        if message.len() > MAX_MESSAGE {
            return Err(FrpError::BufferTooShort);
        }
        let total_chunks = ((message.len() as u16) + chunk_size - 1) / chunk_size;
        if total_chunks as usize > MAX_CHUNKS {
            return Err(FrpError::BufferTooShort);
        }

        let mut msg = Vec::new();
        msg.extend_from_slice(message)
            .map_err(|_| FrpError::BufferTooShort)?;

        let mut tx_queue = Vec::new();
        for i in 0..total_chunks {
            tx_queue.push(i).map_err(|_| FrpError::BufferTooShort)?;
        }

        Ok(Self {
            session_id,
            total_chunks,
            chunk_size,
            checksum,
            message: msg,
            tx_queue,
            tx_cursor: 0,
            start_sent: false,
        })
    }

    /// Serialize next packet into `buf`.
    ///
    /// Returns `None` when all packets sent.  Returns the filled portion
    /// of `buf` on success.  Caller loops until `None`.
    pub fn next_packet<'a>(&mut self, buf: &'a mut [u8]) -> Result<Option<&'a mut [u8]>, FrpError> {
        // 1. Send FrpStart first.
        if !self.start_sent {
            self.start_sent = true;
            let start = FrpStart {
                session_id: self.session_id,
                total_chunks: self.total_chunks,
                chunk_size: self.chunk_size,
                message_length: self.message.len() as u32,
                checksum: self.checksum,
            };
            return start.try_encode(buf).map(Some);
        }

        // 2. Send next chunk from queue.
        if self.tx_cursor < self.tx_queue.len() {
            let chunk_idx = self.tx_queue[self.tx_cursor];
            self.tx_cursor += 1;

            let start = chunk_idx as usize * self.chunk_size as usize;
            let end = core::cmp::min(start + self.chunk_size as usize, self.message.len());
            let payload = Vec::<u8, MAX_CHUNK_PAYLOAD>::from_slice(&self.message[start..end])
                .map_err(|_| FrpError::BufferTooShort)?;

            let data = FrpData {
                session_id: self.session_id,
                chunk_index: chunk_idx,
                flags: 0x00,
                payload,
            };
            return data.try_encode(buf).map(Some);
        }

        Ok(None)
    }

    /// Rebuild `tx_queue` from `FrpCheck`: only chunks NOT yet acked.
    pub fn process_check(&mut self, check: &FrpCheck) -> Result<(), FrpError> {
        if check.session_id != self.session_id {
            return Err(FrpError::InvalidSessionId);
        }

        self.tx_queue.clear();
        self.tx_cursor = 0;

        for i in 0..self.total_chunks {
            let byte_idx = i as usize / 8;
            let bit_idx = i as usize % 8;

            if byte_idx < check.ack_bitmap.len()
                && (check.ack_bitmap[byte_idx] & (1 << bit_idx)) == 0
            {
                self.tx_queue
                    .push(i)
                    .map_err(|_| FrpError::BufferTooShort)?;
            }
        }

        Ok(())
    }

    /// All packets (FrpStart + all queued chunks) sent?
    pub fn is_done(&self) -> bool {
        self.start_sent && self.tx_cursor >= self.tx_queue.len()
    }

    /// Check final flags from FrpCheck.
    pub fn is_session_success(check_flags: u8) -> bool {
        (check_flags & 0x01) != 0 && (check_flags & 0x02) == 0
    }
}

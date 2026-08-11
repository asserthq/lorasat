use crate::error::FrpError;
use crate::messages::{FrpCheck, FrpData, FrpStart};

pub struct ReceiverSession {
    session_id: u16,
    buffer: Vec<u8>,
    bitmap: Vec<u8>,
    total_chunks: u16,
    chunk_size: u16,
    expected_checksum: u32,
    is_initialized: bool,
}

impl ReceiverSession {
    pub fn new() -> Self {
        Self {
            session_id: 0,
            buffer: Vec::new(),
            bitmap: Vec::new(),
            total_chunks: 0,
            chunk_size: 0,
            expected_checksum: 0,
            is_initialized: false,
        }
    }

    pub fn handle_start(&mut self, msg: &FrpStart) {
        self.session_id = msg.session_id;
        self.total_chunks = msg.total_chunks;
        self.chunk_size = msg.chunk_size;
        self.expected_checksum = msg.checksum;

        self.buffer = vec![0; msg.message_length as usize];

        let bitmap_size = (self.total_chunks as usize + 7) / 8;
        self.bitmap = vec![0; bitmap_size];

        self.is_initialized = true;
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

        if end <= self.buffer.len() {
            self.buffer[offset..end].copy_from_slice(&msg.payload);
        }

        let byte_idx = msg.chunk_index as usize / 8;
        let bit_idx = msg.chunk_index as usize % 8;
        self.bitmap[byte_idx] |= 1 << bit_idx;

        Ok(())
    }

    pub fn generate_check(&self) -> Result<FrpCheck, FrpError> {
        if !self.is_initialized {
            return Err(FrpError::SessionNotInitialized);
        }

        let all_received = self.is_bitmap_fully_set();
        let mut flags = 0x00;

        if all_received {
            flags |= 0x01;
        }

        Ok(FrpCheck {
            session_id: self.session_id,
            total_chunks: self.total_chunks,
            flags,
            ack_bitmap: self.bitmap.clone(),
        })
    }

    fn is_bitmap_fully_set(&self) -> bool {
        for i in 0..self.total_chunks {
            let byte_idx = i as usize / 8;
            let bit_idx = i as usize % 8;
            if (self.bitmap[byte_idx] & (1 << bit_idx)) == 0 {
                return false;
            }
        }
        true
    }

    pub fn get_assembled_message(&self) -> Option<Vec<u8>> {
        if self.is_initialized && self.is_bitmap_fully_set() {
            // Тут мб стоит возвращать CRC, но для проверки так
            Some(self.buffer.clone())
        } else {
            None
        }
    }
}

pub struct SenderSession {
    session_id: u16,
    total_chunks: u16,
    chunks: Vec<Vec<u8>>,
    tx_queue: Vec<Vec<u8>>,
}

impl SenderSession {
    pub fn new(message: &[u8], chunk_size: u16, session_id: u16, checksum: u32) -> Self {
        let total_chunks = ((message.len() as u16) + chunk_size - 1) / chunk_size;
        let mut chunks = Vec::with_capacity(total_chunks as usize);
        let mut tx_queue = Vec::new();

        for i in 0..total_chunks {
            let start = i as usize * chunk_size as usize;
            let end = std::cmp::min(start + chunk_size as usize, message.len());
            chunks.push(message[start..end].to_vec());
        }

        let start_msg = FrpStart {
            session_id,
            total_chunks,
            chunk_size,
            message_length: message.len() as u32,
            checksum,
        };
        tx_queue.push(start_msg.to_bytes());

        for (i, payload) in chunks.iter().enumerate() {
            let data_msg = FrpData {
                session_id,
                chunk_index: i as u16,
                flags: 0x00,
                payload: payload.clone(),
            };
            tx_queue.push(data_msg.to_bytes());
        }

        Self {
            session_id,
            total_chunks,
            chunks,
            tx_queue,
        }
    }

    pub fn process_check(&mut self, check_bytes: &[u8]) -> Result<(), FrpError> {
        let check = FrpCheck::parse(check_bytes)?;

        if check.session_id != self.session_id {
            return Err(FrpError::InvalidSessionId);
        }

        self.tx_queue.clear();

        for i in 0..self.total_chunks {
            let byte_idx = i as usize / 8;
            let bit_idx = i as usize % 8;

            if (check.ack_bitmap[byte_idx] & (1 << bit_idx)) == 0 {
                let data_msg = FrpData {
                    session_id: self.session_id,
                    chunk_index: i,
                    flags: 0x00,
                    payload: self.chunks[i as usize].clone(), // Берем готовый кусок из памяти
                };
                self.tx_queue.push(data_msg.to_bytes());
            }
        }

        Ok(())
    }

    pub fn get_packets_to_send(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.tx_queue)
    }

    pub fn is_session_success(check_flags: u8) -> bool {
        (check_flags & 0x01) != 0 && (check_flags & 0x02) == 0
    }
}

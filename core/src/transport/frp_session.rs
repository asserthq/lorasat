use super::frp_error::FrpError;
use super::frp_message::*;
use heapless::Vec;

pub struct ReceiverSession {
    session_id: u16,
    buffer: Vec<u8, 256>,
    bitmap: Vec<u8, 256>,
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

        self.buffer = Vec::from_array([0; msg.message_length as usize]);

        let bitmap_size = (self.total_chunks as usize + 7) / 8;
        self.bitmap = Vec::from_array([0; bitmap_size]);

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

    pub fn get_assembled_message(&self) -> Option<Vec<u8, 256>> {
        if self.is_initialized && self.is_bitmap_fully_set() {
            // Тут мб стоит возвращать CRC, но для проверки так
            Some(self.buffer.clone())
        } else {
            None
        }
    }

    //fn is_complete(&self) -> bool; // все чанки получены?
}

// /// Состояние отправителя: очередь чанков на отправку, ретрансмиты.
// struct SenderSession {
//     session_id: u16,
//     total_chunks: u16,
//     chunks: [u8; MAX_MESSAGE_SIZE], // копия всего сообщения (для ретрансмитов)
//     chunk_size: u16,
//     tx_queue: [u16; MAX_CHUNKS], // очередь индексов чанков на отправку
//     tx_queue_len: usize,
// }

// impl SenderSession {
//     fn new(message: &[u8], chunk_size: u16, session_id: u16) -> Self;
//     fn next_packet(&mut self) -> Option<Vec<u8>>; // следующий FrpStart или FrpData
//     fn process_check(&mut self, check: &FrpCheck); // перестроить очередь ретрансмитов
//     fn is_done(&self) -> bool;
// }

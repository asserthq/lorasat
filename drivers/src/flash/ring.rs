use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;

use super::error::Error;
use super::{CAPACITY, SECTOR_SIZE, W25Q64};

pub const HEADER_SIZE: usize = 20;
pub const MAGIC: u16 = 0xA55A;
pub const MAX_PAYLOAD: usize = (SECTOR_SIZE as usize) - HEADER_SIZE;

const MAGIC_OFF: usize = 0;
const LEN_OFF: usize = 2;
const SEQ_OFF: usize = 4;
const TIMESTAMP_OFF: usize = 8;
const CRC_OFF: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordHeader {
    pub seq: u32,
    pub timestamp: u64,
    pub payload_len: usize,
}

pub struct Record<'a> {
    pub seq: u32,
    pub timestamp: u64,
    pub data: &'a [u8],
}

pub struct FlashRing<SPI, DLY>
where
    SPI: SpiDevice<u8>,
    DLY: DelayNs,
{
    flash: W25Q64<SPI, DLY>,
    base: u32,
    len: u32,
    head: u32,
    tail: u32,
    seq: u32,
    read_pos: Option<u32>,
}

impl<SPI, DLY> FlashRing<SPI, DLY>
where
    SPI: SpiDevice<u8>,
    DLY: DelayNs,
{
    pub fn new(flash: W25Q64<SPI, DLY>, base: u32, len: u32) -> Result<Self, Error> {
        if len == 0 || len % SECTOR_SIZE != 0 {
            return Err(Error::BadRegion);
        }
        if base.checked_add(len).map_or(true, |end| end > CAPACITY) {
            return Err(Error::InvalidAddress);
        }
        Ok(Self {
            flash,
            base,
            len,
            head: 0,
            tail: 0,
            seq: 1,
            read_pos: None,
        })
    }

    pub fn recover(&mut self) -> Result<(), Error> {
        let mut offset = self.base;
        let mut min_seq = u32::MAX;
        let mut min_seq_off: Option<u32> = None;
        let mut max_seq: u32 = 0;
        let mut max_seq_end: Option<u32> = None;

        while offset < self.base + self.len {
            let mut hdr_buf = [0u8; HEADER_SIZE];
            self.flash.read(offset, &mut hdr_buf)?;

            if is_erased(&hdr_buf) {
                let next = align_up(offset + 1, SECTOR_SIZE);
                if next <= offset || next >= self.base + self.len {
                    break;
                }
                offset = next;
                continue;
            }

            if read_u16(&hdr_buf, MAGIC_OFF) != MAGIC {
                break;
            }

            let len = read_u16(&hdr_buf, LEN_OFF) as usize;
            if len > MAX_PAYLOAD || offset + HEADER_SIZE as u32 + len as u32 > self.base + self.len
            {
                break;
            }

            let seq = read_u32(&hdr_buf, SEQ_OFF);
            let expected_crc = read_u32(&hdr_buf, CRC_OFF);

            let mut crc = Crc32::new();
            let mut p = offset + HEADER_SIZE as u32;
            let mut remaining = len;
            while remaining > 0 {
                let mut chunk = [0u8; 64];
                let n = remaining.min(chunk.len());
                self.flash.read(p, &mut chunk[..n])?;
                crc.update(&chunk[..n]);
                p += n as u32;
                remaining -= n;
            }

            if crc.finalize() != expected_crc {
                break;
            }

            if seq < min_seq {
                min_seq = seq;
                min_seq_off = Some(offset);
            }
            if seq >= max_seq {
                max_seq = seq;
                max_seq_end = Some(offset + HEADER_SIZE as u32 + len as u32);
            }

            offset += HEADER_SIZE as u32 + len as u32;
        }

        self.head = min_seq_off.unwrap_or(self.base);
        self.tail = max_seq_end.unwrap_or(self.base);
        self.seq = if min_seq_off.is_some() {
            max_seq.wrapping_add(1)
        } else {
            1
        };
        self.read_pos = Some(self.head);
        Ok(())
    }

    pub fn append(&mut self, payload: &[u8], timestamp: u64) -> Result<(), Error> {
        if payload.len() > MAX_PAYLOAD {
            return Err(Error::PayloadTooLong);
        }

        let rec_size = HEADER_SIZE + payload.len();
        let mut addr = self.base + self.tail;

        let sector_start = addr & !(SECTOR_SIZE - 1);
        let sector_end = sector_start + SECTOR_SIZE;

        if addr + rec_size as u32 > sector_end {
            let mut next = sector_end;
            if next >= self.base + self.len {
                next = self.base;
            }
            self.flash.erase_sector(next)?;
            addr = next;
        }

        let mut header = [0u8; HEADER_SIZE];
        write_u16(&mut header, MAGIC_OFF, MAGIC);
        write_u16(&mut header, LEN_OFF, payload.len() as u16);
        write_u32(&mut header, SEQ_OFF, self.seq);
        write_u64(&mut header, TIMESTAMP_OFF, timestamp);
        write_u32(&mut header, CRC_OFF, crc32(payload));

        self.flash.program(addr, &header)?;
        self.flash.program(addr + HEADER_SIZE as u32, payload)?;

        self.tail = (addr - self.base) + rec_size as u32;
        self.seq = self.seq.wrapping_add(1);
        Ok(())
    }

    pub fn rewind(&mut self) {
        self.read_pos = Some(self.head);
    }

    pub fn read_next<'a>(&mut self, buf: &'a mut [u8]) -> Result<Option<Record<'a>>, Error> {
        loop {
            if self.head == self.tail {
                self.read_pos = None;
                return Ok(None);
            }

            let off = match self.read_pos {
                Some(o) => o,
                None => return Ok(None),
            };

            let mut hdr_buf = [0u8; HEADER_SIZE];
            self.flash.read(off, &mut hdr_buf)?;

            if is_erased(&hdr_buf) {
                let mut next = align_up(off + 1, SECTOR_SIZE);
                if next >= self.base + self.len {
                    next = self.base;
                }
                if next == self.tail {
                    self.read_pos = None;
                    return Ok(None);
                }
                self.read_pos = Some(next);
                continue;
            }

            let rec = self.read_record_at(off, buf)?;
            self.read_pos = self.next_offset(off)?;
            return Ok(Some(rec));
        }
    }

    pub fn head(&self) -> u32 {
        self.base + self.head
    }

    pub fn tail(&self) -> u32 {
        self.base + self.tail
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    fn read_record_at<'a>(&mut self, offset: u32, buf: &'a mut [u8]) -> Result<Record<'a>, Error> {
        let hdr = self.read_header(offset)?;
        let len = hdr.payload_len;
        if buf.len() < len {
            return Err(Error::BufferTooSmall);
        }
        self.flash
            .read(offset + HEADER_SIZE as u32, &mut buf[..len])?;
        Ok(Record {
            seq: hdr.seq,
            timestamp: hdr.timestamp,
            data: &buf[..len],
        })
    }

    fn read_header(&mut self, offset: u32) -> Result<RecordHeader, Error> {
        let mut buf = [0u8; HEADER_SIZE];
        self.flash.read(offset, &mut buf)?;
        Ok(RecordHeader {
            seq: read_u32(&buf, SEQ_OFF),
            timestamp: read_u64(&buf, TIMESTAMP_OFF),
            payload_len: read_u16(&buf, LEN_OFF) as usize,
        })
    }

    fn next_offset(&mut self, offset: u32) -> Result<Option<u32>, Error> {
        let hdr = self.read_header(offset)?;
        let mut next = offset + HEADER_SIZE as u32 + hdr.payload_len as u32;
        if next >= self.base + self.len {
            next = self.base;
        }
        if next == self.tail {
            Ok(None)
        } else {
            Ok(Some(next))
        }
    }
}

fn align_up(addr: u32, align: u32) -> u32 {
    (addr + align - 1) & !(align - 1)
}

fn is_erased(buf: &[u8]) -> bool {
    buf.iter().all(|&b| b == 0xFF)
}

fn read_u16(buf: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([buf[off], buf[off + 1]])
}

fn read_u32(buf: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
}

fn read_u64(buf: &[u8], off: usize) -> u64 {
    u64::from_le_bytes([
        buf[off],
        buf[off + 1],
        buf[off + 2],
        buf[off + 3],
        buf[off + 4],
        buf[off + 5],
        buf[off + 6],
        buf[off + 7],
    ])
}

fn write_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

fn write_u32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn write_u64(buf: &mut [u8], off: usize, v: u64) {
    buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

pub struct Crc32(u32);

impl Crc32 {
    pub fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut crc = self.0;
        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        self.0 = crc;
    }

    pub fn finalize(self) -> u32 {
        self.0 ^ 0xFFFF_FFFF
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(data);
    crc.finalize()
}

/*
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn crc32_streaming_matches_oneshot() {
        let data = b"streaming crc over multiple chunks";
        let mut crc = Crc32::new();
        crc.update(&data[..10]);
        crc.update(&data[10..]);
        assert_eq!(crc.finalize(), crc32(data));
    }

    #[test]
    fn header_roundtrip() {
        let mut buf = [0u8; HEADER_SIZE];
        write_u16(&mut buf, MAGIC_OFF, MAGIC);
        write_u16(&mut buf, LEN_OFF, 42);
        write_u32(&mut buf, SEQ_OFF, 0xDEAD_BEEF);
        write_u64(&mut buf, TIMESTAMP_OFF, 0x0102_0304_0506_0708);
        write_u32(&mut buf, CRC_OFF, 0xCAFE_BABE);

        assert_eq!(read_u16(&buf, MAGIC_OFF), MAGIC);
        assert_eq!(read_u16(&buf, LEN_OFF), 42);
        assert_eq!(read_u32(&buf, SEQ_OFF), 0xDEAD_BEEF);
        assert_eq!(read_u64(&buf, TIMESTAMP_OFF), 0x0102_0304_0506_0708);
        assert_eq!(read_u32(&buf, CRC_OFF), 0xCAFE_BABE);
    }

    #[test]
    fn erased_detection() {
        assert!(is_erased(&[0xFF; HEADER_SIZE]));
        assert!(!is_erased(&[0x00; HEADER_SIZE]));
        assert!(!is_erased(&[0xFF, 0x00, 0xFF, 0xFF]));
    }
}
*/

pub mod error;
pub mod ring;

pub use error::Error;

use embedded_hal::delay::DelayNs;
use embedded_hal::spi::{Operation, SpiDevice};

pub const PAGE_SIZE: u32 = 256;
pub const SECTOR_SIZE: u32 = 4096;
pub const CAPACITY: u32 = 8 * 1024 * 1024;

const CMD_READ_JEDEC_ID: u8 = 0x9F;
const CMD_READ_DATA: u8 = 0x03;
const CMD_WRITE_ENABLE: u8 = 0x06;
const CMD_PAGE_PROGRAM: u8 = 0x02;
const CMD_SECTOR_ERASE: u8 = 0x20;
const CMD_CHIP_ERASE: u8 = 0xC7;
const CMD_READ_STATUS_1: u8 = 0x05;

const STATUS_BUSY: u8 = 0x01;
const MAX_POLL_ITER: u32 = 10_000;

pub struct W25Q64<SPI, DLY>
where
    SPI: SpiDevice<u8>,
    DLY: DelayNs,
{
    spi: SPI,
    delay: DLY,
}

impl<SPI, DLY> W25Q64<SPI, DLY>
where
    SPI: SpiDevice<u8>,
    DLY: DelayNs,
{
    pub fn new(spi: SPI, delay: DLY) -> Self {
        Self { spi, delay }
    }

    pub fn read_id(&mut self) -> Result<[u8; 3], Error> {
        let mut buf = [CMD_READ_JEDEC_ID, 0x00, 0x00, 0x00];
        self.spi
            .transfer_in_place(&mut buf)
            .map_err(|_| Error::Spi)?;
        Ok([buf[1], buf[2], buf[3]])
    }

    pub fn read(&mut self, addr: u32, out: &mut [u8]) -> Result<(), Error> {
        if out.is_empty() {
            return Ok(());
        }
        self.check_addr(addr)?;
        self.check_addr(addr + out.len() as u32 - 1)?;

        let head = [
            CMD_READ_DATA,
            (addr >> 16) as u8,
            (addr >> 8) as u8,
            addr as u8,
        ];
        let mut ops = [Operation::Write(&head), Operation::Read(out)];
        self.spi.transaction(&mut ops).map_err(|_| Error::Spi)?;
        Ok(())
    }

    pub fn write_enable(&mut self) -> Result<(), Error> {
        self.spi
            .write(&[CMD_WRITE_ENABLE])
            .map_err(|_| Error::Spi)?;
        Ok(())
    }

    pub fn program(&mut self, addr: u32, data: &[u8]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }
        self.check_addr(addr)?;
        self.check_addr(addr + data.len() as u32 - 1)?;

        let mut off = 0usize;
        while off < data.len() {
            let page_off = (addr + off as u32) % PAGE_SIZE;
            let chunk = (PAGE_SIZE - page_off).min((data.len() - off) as u32) as usize;
            self.program_page(addr + off as u32, &data[off..off + chunk])?;
            off += chunk;
        }
        Ok(())
    }

    pub fn erase_sector(&mut self, addr: u32) -> Result<(), Error> {
        self.check_addr(addr)?;
        if addr % SECTOR_SIZE != 0 {
            return Err(Error::UnalignedAddress);
        }
        self.write_enable()?;
        let head = [
            CMD_SECTOR_ERASE,
            (addr >> 16) as u8,
            (addr >> 8) as u8,
            addr as u8,
        ];
        self.spi.write(&head).map_err(|_| Error::Spi)?;
        self.wait_ready()
    }

    pub fn erase_chip(&mut self) -> Result<(), Error> {
        self.write_enable()?;
        self.spi.write(&[CMD_CHIP_ERASE]).map_err(|_| Error::Spi)?;
        self.wait_ready()
    }

    pub fn is_busy(&mut self) -> Result<bool, Error> {
        let mut buf = [CMD_READ_STATUS_1, 0x00];
        self.spi
            .transfer_in_place(&mut buf)
            .map_err(|_| Error::Spi)?;
        Ok(buf[1] & STATUS_BUSY != 0)
    }

    pub fn wait_ready(&mut self) -> Result<(), Error> {
        for _ in 0..MAX_POLL_ITER {
            if !self.is_busy()? {
                return Ok(());
            }
            self.delay.delay_us(100);
        }
        Err(Error::Timeout)
    }

    fn program_page(&mut self, addr: u32, data: &[u8]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }
        if data.len() as u32 > PAGE_SIZE - (addr % PAGE_SIZE) {
            return Err(Error::PageOverflow);
        }

        self.write_enable()?;
        let head = [
            CMD_PAGE_PROGRAM,
            (addr >> 16) as u8,
            (addr >> 8) as u8,
            addr as u8,
        ];
        let mut ops = [Operation::Write(&head), Operation::Write(data)];
        self.spi.transaction(&mut ops).map_err(|_| Error::Spi)?;
        self.wait_ready()
    }

    fn check_addr(&self, addr: u32) -> Result<(), Error> {
        if addr >= CAPACITY {
            return Err(Error::InvalidAddress);
        }
        Ok(())
    }
}

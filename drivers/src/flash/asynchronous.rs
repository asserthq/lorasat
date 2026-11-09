use embedded_hal_async::{
    delay::DelayNs,
    spi::{Operation, SpiDevice},
};
use embedded_storage_async::nor_flash::{
    ErrorType, MultiwriteNorFlash, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};

use super::{
    CAPACITY, CMD_PAGE_PROGRAM, CMD_READ_DATA, CMD_READ_JEDEC_ID, CMD_READ_STATUS_1,
    CMD_SECTOR_ERASE, CMD_WRITE_ENABLE, MAX_POLL_ITER, PAGE_SIZE, SECTOR_SIZE, STATUS_BUSY,
};

const STATUS_WEL: u8 = 0x02;

#[derive(Debug, PartialEq, Eq)]
pub enum AsyncFlashError<SpiError> {
    Spi(SpiError),

    Timeout,
    OutOfBounds,
    NotAligned,

    WriteDisabled,
    UnsupportedDevice([u8; 3]),
}

impl<SpiError: core::fmt::Debug> NorFlashError for AsyncFlashError<SpiError> {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Self::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            Self::NotAligned => NorFlashErrorKind::NotAligned,
            _ => NorFlashErrorKind::Other,
        }
    }
}

pub struct AsyncW25Q64<Spi, Delay> {
    spi: Spi,
    delay: Delay,
}

impl<Spi, Delay> AsyncW25Q64<Spi, Delay>
where
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    pub async fn new(spi: Spi, delay: Delay) -> Result<Self, AsyncFlashError<Spi::Error>> {
        let mut flash = Self { spi, delay };
        let id = flash.read_jedec_id().await?;
        if !matches!(id, [0xEF, 0x40, 0x17] | [0xEF, 0x60, 0x17]) {
            return Err(AsyncFlashError::UnsupportedDevice(id));
        }
        Ok(flash)
    }

    pub async fn read_jedec_id(&mut self) -> Result<[u8; 3], AsyncFlashError<Spi::Error>> {
        self.wait_ready().await?;
        let mut id = [0; 3];
        self.spi
            .transaction(&mut [
                Operation::Write(&[CMD_READ_JEDEC_ID]),
                Operation::Read(&mut id),
            ])
            .await
            .map_err(AsyncFlashError::Spi)?;
        Ok(id)
    }

    pub fn into_inner(self) -> (Spi, Delay) {
        (self.spi, self.delay)
    }

    fn check_range(offset: u32, length: usize) -> Result<(), AsyncFlashError<Spi::Error>> {
        let length = u32::try_from(length).map_err(|_| AsyncFlashError::OutOfBounds)?;
        match offset.checked_add(length) {
            Some(end) if end <= CAPACITY => Ok(()),
            _ => Err(AsyncFlashError::OutOfBounds),
        }
    }

    async fn read_status(&mut self) -> Result<u8, AsyncFlashError<Spi::Error>> {
        let mut status = [0];
        self.spi
            .transaction(&mut [
                Operation::Write(&[CMD_READ_STATUS_1]),
                Operation::Read(&mut status),
            ])
            .await
            .map_err(AsyncFlashError::Spi)?;
        Ok(status[0])
    }

    async fn wait_ready(&mut self) -> Result<(), AsyncFlashError<Spi::Error>> {
        for _ in 0..MAX_POLL_ITER {
            if self.read_status().await? & STATUS_BUSY == 0 {
                return Ok(());
            }
            self.delay.delay_us(100).await;
        }
        Err(AsyncFlashError::Timeout)
    }

    async fn write_enable(&mut self) -> Result<(), AsyncFlashError<Spi::Error>> {
        self.spi
            .write(&[CMD_WRITE_ENABLE])
            .await
            .map_err(AsyncFlashError::Spi)?;
        if self.read_status().await? & STATUS_WEL == 0 {
            return Err(AsyncFlashError::WriteDisabled);
        }
        Ok(())
    }

    fn address_command(command: u8, address: u32) -> [u8; 4] {
        [
            command,
            (address >> 16) as u8,
            (address >> 8) as u8,
            address as u8,
        ]
    }
}

impl<Spi, Delay> ErrorType for AsyncW25Q64<Spi, Delay>
where
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    type Error = AsyncFlashError<Spi::Error>;
}

impl<Spi, Delay> ReadNorFlash for AsyncW25Q64<Spi, Delay>
where
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    const READ_SIZE: usize = 1;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        Self::check_range(offset, bytes.len())?;
        self.wait_ready().await?;
        if bytes.is_empty() {
            return Ok(());
        }
        let command = Self::address_command(CMD_READ_DATA, offset);
        self.spi
            .transaction(&mut [Operation::Write(&command), Operation::Read(bytes)])
            .await
            .map_err(AsyncFlashError::Spi)
    }

    fn capacity(&self) -> usize {
        CAPACITY as usize
    }
}

impl<Spi, Delay> NorFlash for AsyncW25Q64<Spi, Delay>
where
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = SECTOR_SIZE as usize;

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        Self::check_range(offset, bytes.len())?;
        self.wait_ready().await?;
        let mut written = 0;
        while written < bytes.len() {
            let address = offset + written as u32;
            let length = ((PAGE_SIZE - address % PAGE_SIZE) as usize).min(bytes.len() - written);
            self.write_enable().await?;
            let command = Self::address_command(CMD_PAGE_PROGRAM, address);
            self.spi
                .transaction(&mut [
                    Operation::Write(&command),
                    Operation::Write(&bytes[written..written + length]),
                ])
                .await
                .map_err(AsyncFlashError::Spi)?;
            self.wait_ready().await?;
            written += length;
        }
        Ok(())
    }

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        let length = to.checked_sub(from).ok_or(AsyncFlashError::OutOfBounds)?;
        Self::check_range(from, length as usize)?;
        if !from.is_multiple_of(SECTOR_SIZE) || !to.is_multiple_of(SECTOR_SIZE) {
            return Err(AsyncFlashError::NotAligned);
        }
        self.wait_ready().await?;
        let mut address = from;
        while address < to {
            self.write_enable().await?;
            self.spi
                .write(&Self::address_command(CMD_SECTOR_ERASE, address))
                .await
                .map_err(AsyncFlashError::Spi)?;
            self.wait_ready().await?;
            address += SECTOR_SIZE;
        }
        Ok(())
    }
}

impl<Spi, Delay> MultiwriteNorFlash for AsyncW25Q64<Spi, Delay>
where
    Spi: SpiDevice<u8>,
    Delay: DelayNs,
{
}

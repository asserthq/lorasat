pub mod error;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiDevice;
use embedded_sdmmc::{Mode, SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};

use self::error::Error;

/// Заглушка
struct StaticTimeSource;

impl TimeSource for StaticTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp::from_calendar(2026, 1, 1, 0, 0, 0).unwrap()
    }
}

const LOG_FILE: &str = "RXLOG.BIN";

pub struct SdCardLogger<SPI, CS, DELAY>
where
    SPI: SpiDevice<u8>,
    CS: OutputPin,
    DELAY: DelayNs,
{
    volume_mgr: VolumeManager<SdCard<SPI, CS, DELAY>, StaticTimeSource>,
}

impl<SPI, CS, DELAY> SdCardLogger<SPI, CS, DELAY>
where
    SPI: SpiDevice<u8>,
    CS: OutputPin,
    DELAY: DelayNs,
{
    pub fn new(spi: SPI, cs: CS, delay: DELAY) -> Self {
        let sd = SdCard::new(spi, cs, delay);
        let volume_mgr = VolumeManager::new(sd, StaticTimeSource);
        Self { volume_mgr }
    }

    pub fn append(&mut self, data: &[u8]) -> Result<(), Error> {
        let mut volume = self
            .volume_mgr
            .open_volume(VolumeIdx(0))
            .map_err(|_| Error::OpenVolume)?;

        let mut root = volume.open_root_dir().map_err(|_| Error::OpenRootDir)?;

        let mut file = root
            .open_file_in_dir(LOG_FILE, Mode::ReadWriteCreateOrAppend)
            .map_err(|_| Error::OpenFile)?;

        file.write(data).map_err(|_| Error::Write)?;

        Ok(())
    }
}

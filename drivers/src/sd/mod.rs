pub mod error;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiDevice;
use embedded_sdmmc::sdcard::CardType;
use embedded_sdmmc::{
    DirEntry, FilenameError, Mode, RawDirectory, RawFile, RawVolume, SdCard, ShortFileName,
    TimeSource, Timestamp, VolumeIdx, VolumeManager,
};

use self::error::Error;

#[derive(Debug, Clone, Copy)]
pub struct StaticTimeSource {
    timestamp: Timestamp,
}

impl StaticTimeSource {
    pub fn new(timestamp: Timestamp) -> Self {
        Self { timestamp }
    }
}

impl Default for StaticTimeSource {
    fn default() -> Self {
        Self {
            timestamp: Timestamp::from_calendar(2026, 1, 1, 0, 0, 0).unwrap(),
        }
    }
}

impl TimeSource for StaticTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        self.timestamp
    }
}

const DEFAULT_FILE_NAME: &str = "RXLOG.BIN";
const ROTATION_STEM: &[u8; 5] = b"RXLOG";
const ROTATION_EXT: &[u8; 3] = b"BIN";
const MAX_ROTATION_INDEX: u16 = 999;

pub struct SdCardLogger<SPI, CS, DELAY, TIME>
where
    SPI: SpiDevice<u8>,
    CS: OutputPin,
    DELAY: DelayNs,
    TIME: TimeSource,
{
    volume_mgr: VolumeManager<SdCard<SPI, CS, DELAY>, TIME>,
    volume: Option<RawVolume>,
    root: Option<RawDirectory>,
    file: Option<RawFile>,
    file_name: ShortFileName,
    file_size: u32,
    max_file_size: u32,
    rotation_index: u16,
    auto_flush: bool,
    max_total_size: u64,
}

impl<SPI, CS, DELAY, TIME> SdCardLogger<SPI, CS, DELAY, TIME>
where
    SPI: SpiDevice<u8>,
    CS: OutputPin,
    DELAY: DelayNs,
    TIME: TimeSource,
{
    pub fn new(spi: SPI, cs: CS, delay: DELAY, time_source: TIME) -> Self {
        let sd = SdCard::new(spi, cs, delay);
        let volume_mgr = VolumeManager::new(sd, time_source);

        Self {
            volume_mgr,
            volume: None,
            root: None,
            file: None,
            file_name: ShortFileName::create_from_str(DEFAULT_FILE_NAME).unwrap(),
            file_size: 0,
            max_file_size: 0,
            rotation_index: 0,
            auto_flush: true,
            max_total_size: 0,
        }
    }

    pub fn init(&mut self) -> Result<(), Error> {
        if self.volume.is_some() {
            return Err(Error::AlreadyInitialized);
        }

        let volume = self.volume_mgr.open_raw_volume(VolumeIdx(0))?;
        let root = self.volume_mgr.open_root_dir(volume)?;
        self.volume = Some(volume);
        self.root = Some(root);

        self.open_file()
    }

    pub fn append(&mut self, data: &[u8]) -> Result<(), Error> {
        if self.max_file_size > 0
            && self.file_size.saturating_add(data.len() as u32) > self.max_file_size
        {
            self.rotate()?;
        }

        let file = self.file.ok_or(Error::NotInitialized)?;

        self.volume_mgr.file_seek_from_end(file, 0)?;
        self.volume_mgr.write(file, data)?;

        self.file_size = self.file_size.saturating_add(data.len() as u32);

        if self.auto_flush {
            self.flush()?;
        }
        self.enforce_retention()?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), Error> {
        self.close_file()?;
        self.open_file()
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        let file = self.file.ok_or(Error::NotInitialized)?;
        Ok(self.volume_mgr.read(file, buffer)?)
    }

    pub fn seek_from_start(&mut self, offset: u32) -> Result<(), Error> {
        let file = self.file.ok_or(Error::NotInitialized)?;
        self.volume_mgr.file_seek_from_start(file, offset)?;
        Ok(())
    }

    pub fn seek_from_end(&mut self, offset: u32) -> Result<(), Error> {
        let file = self.file.ok_or(Error::NotInitialized)?;
        self.volume_mgr.file_seek_from_end(file, offset)?;
        Ok(())
    }

    pub fn seek_from_current(&mut self, offset: i32) -> Result<(), Error> {
        let file = self.file.ok_or(Error::NotInitialized)?;
        self.volume_mgr.file_seek_from_current(file, offset)?;
        Ok(())
    }

    pub fn file_size(&mut self) -> Result<u32, Error> {
        let file = self.file.ok_or(Error::NotInitialized)?;
        Ok(self.volume_mgr.file_length(file)?)
    }

    pub fn for_each_entry<F>(&mut self, func: F) -> Result<(), Error>
    where
        F: FnMut(&DirEntry),
    {
        let root = self.root.ok_or(Error::NotInitialized)?;
        self.volume_mgr.iterate_dir(root, func)?;
        Ok(())
    }

    pub fn card_size_bytes(&mut self) -> Result<u64, Error> {
        Ok(self
            .volume_mgr
            .device()
            .num_bytes()
            .map_err(Error::Device)?)
    }

    pub fn card_type(&mut self) -> Option<CardType> {
        self.volume_mgr.device().get_card_type()
    }

    pub fn set_file_name(&mut self, name: &str) -> Result<(), Error> {
        let file_name = ShortFileName::create_from_str(name).map_err(Error::Filename)?;
        self.file_name = file_name;
        self.max_file_size = 0;

        if self.file.is_some() {
            self.close_file()?;
            self.open_file()?;
        }
        Ok(())
    }

    pub fn enable_rotation(&mut self, max_file_size: u32) -> Result<(), Error> {
        self.max_file_size = max_file_size;
        self.rotation_index = 0;
        self.file_name = Self::rotated_name(0)?;

        if self.file.is_some() {
            self.close_file()?;
            self.open_file()?;
        }
        Ok(())
    }

    pub fn disable_rotation(&mut self) {
        self.max_file_size = 0;
    }

    pub fn set_auto_flush(&mut self, enabled: bool) {
        self.auto_flush = enabled;
    }

    pub fn set_max_total_size(&mut self, max_total_size: u64) {
        self.max_total_size = max_total_size;
    }

    pub fn total_log_size(&mut self) -> Result<u64, Error> {
        let mut total: u64 = 0;
        self.for_each_entry(|entry| {
            if Self::parse_rotated_name(&entry.name).is_some() {
                total += u64::from(entry.size);
            }
        })?;
        Ok(total)
    }

    pub fn delete_oldest(&mut self) -> Result<bool, Error> {
        let current_name = self.file_name.clone();
        let mut oldest: Option<(u16, ShortFileName)> = None;

        self.for_each_entry(|entry| {
            if entry.name == current_name {
                return;
            }
            if let Some(index) = Self::parse_rotated_name(&entry.name) {
                if oldest.as_ref().map_or(true, |(i, _)| index < *i) {
                    oldest = Some((index, entry.name.clone()));
                }
            }
        })?;

        if let Some((_, name)) = oldest {
            let root = self.root.ok_or(Error::NotInitialized)?;
            self.volume_mgr.delete_file_in_dir(root, &name)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn enforce_retention(&mut self) -> Result<(), Error> {
        while self.max_total_size > 0 && self.total_log_size()? > self.max_total_size {
            if !self.delete_oldest()? {
                break;
            }
        }
        Ok(())
    }

    pub fn highest_rotation_index(&mut self) -> Result<Option<u16>, Error> {
        let mut highest: Option<u16> = None;
        self.for_each_entry(|entry| {
            if let Some(index) = Self::parse_rotated_name(&entry.name) {
                if highest.map_or(true, |h| index > h) {
                    highest = Some(index);
                }
            }
        })?;
        Ok(highest)
    }

    pub fn resume_rotation(&mut self, max_file_size: u32) -> Result<(), Error> {
        let index = self.highest_rotation_index()?.unwrap_or(0);

        self.max_file_size = max_file_size;
        self.rotation_index = index;
        self.file_name = Self::rotated_name(index)?;

        if self.file.is_some() {
            self.close_file()?;
            self.open_file()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> Result<(), Error> {
        self.close_file()?;

        if self.rotation_index >= MAX_ROTATION_INDEX {
            return Err(Error::MaxFilesReached);
        }
        self.rotation_index += 1;
        self.file_name = Self::rotated_name(self.rotation_index)?;
        self.file_size = 0;

        self.open_file()
    }

    fn close_file(&mut self) -> Result<(), Error> {
        if let Some(file) = self.file.take() {
            self.volume_mgr.close_file(file)?;
        }
        Ok(())
    }

    fn open_file(&mut self) -> Result<(), Error> {
        let root = self.root.ok_or(Error::NotInitialized)?;
        let file = self.volume_mgr.open_file_in_dir(
            root,
            &self.file_name,
            Mode::ReadWriteCreateOrAppend,
        )?;
        self.file = Some(file);
        self.file_size = self.volume_mgr.file_length(file)?;
        Ok(())
    }

    fn rotated_name(index: u16) -> Result<ShortFileName, Error> {
        let index = index % 1000;
        let mut buffer = [0u8; 12];

        buffer[0..5].copy_from_slice(ROTATION_STEM);
        buffer[5] = b'0' + ((index / 100) % 10) as u8;
        buffer[6] = b'0' + ((index / 10) % 10) as u8;
        buffer[7] = b'0' + (index % 10) as u8;
        buffer[8] = b'.';
        buffer[9..12].copy_from_slice(ROTATION_EXT);

        let name =
            core::str::from_utf8(&buffer).map_err(|_| Error::Filename(FilenameError::Utf8Error))?;
        ShortFileName::create_from_str(name).map_err(Error::Filename)
    }

    fn parse_rotated_name(name: &ShortFileName) -> Option<u16> {
        if name.extension() != &ROTATION_EXT[..] {
            return None;
        }

        let base = name.base_name();
        if base.len() != 8 || &base[0..5] != &ROTATION_STEM[..] {
            return None;
        }

        let hundreds = base[5].checked_sub(b'0')? as u16;
        let tens = base[6].checked_sub(b'0')? as u16;
        let ones = base[7].checked_sub(b'0')? as u16;
        if hundreds > 9 || tens > 9 || ones > 9 {
            return None;
        }

        Some(hundreds * 100 + tens * 10 + ones)
    }
}

use defmt::info;
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;

use embassy_time::Instant;

use sat_core::storage::Logger;
use sat_drivers::flash::Error;
use sat_drivers::flash::ring::FlashRing;

pub struct FlashLogger<SPI, DLY>
where
    SPI: SpiDevice<u8>,
    DLY: DelayNs,
{
    ring: FlashRing<SPI, DLY>,
}

impl<SPI, DLY> FlashLogger<SPI, DLY>
where
    SPI: SpiDevice<u8>,
    DLY: DelayNs,
{
    pub fn new(ring: FlashRing<SPI, DLY>) -> Self {
        Self { ring }
    }

    pub fn recover(&mut self) -> Result<(), Error> {
        self.ring.recover()
    }

    pub fn dump(&mut self) -> Result<(), Error> {
        self.ring.rewind();
        let mut buf = [0u8; 4096];
        loop {
            match self.ring.read_next(&mut buf)? {
                Some(rec) => info!(
                    "flash: seq={} ts={} data={:?}",
                    rec.seq, rec.timestamp, rec.data
                ),
                None => break,
            }
        }
        Ok(())
    }

    pub fn read_next<'a>(
        &mut self,
        buf: &'a mut [u8],
    ) -> Result<Option<sat_drivers::flash::ring::Record<'a>>, Error> {
        self.ring.read_next(buf)
    }
}

impl<SPI, DLY> Logger for FlashLogger<SPI, DLY>
where
    SPI: SpiDevice<u8>,
    DLY: DelayNs,
{
    type Error = Error;

    fn append(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let ts: u64 = Instant::now().as_millis();
        self.ring.append(data, ts)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

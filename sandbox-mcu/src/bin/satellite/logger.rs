use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::{ErrorType, Operation, SpiDevice};

pub struct BitbangSpiDevice<SCK, MOSI, MISO, DELAY>
where
    SCK: OutputPin,
    MOSI: OutputPin,
    MISO: InputPin,
    DELAY: DelayNs,
{
    sck: SCK,
    mosi: MOSI,
    miso: MISO,
    delay: DELAY,
}

impl<SCK, MOSI, MISO, DELAY> BitbangSpiDevice<SCK, MOSI, MISO, DELAY>
where
    SCK: OutputPin,
    MOSI: OutputPin,
    MISO: InputPin,
    DELAY: DelayNs,
{
    pub fn new(sck: SCK, mosi: MOSI, miso: MISO, delay: DELAY) -> Self {
        Self {
            sck,
            mosi,
            miso,
            delay,
        }
    }

    fn transfer_byte(&mut self, byte: u8) -> u8 {
        let mut out = byte;
        let mut read = 0u8;

        for _ in 0..8 {
            if out & 0x80 != 0 {
                self.mosi.set_high().ok();
            } else {
                self.mosi.set_low().ok();
            }
            self.sck.set_high().ok();

            read = (read << 1)
                | if self.miso.is_high().unwrap_or(false) {
                    1
                } else {
                    0
                };

            self.sck.set_low().ok();
            out <<= 1;
        }

        read
    }
}

impl<SCK, MOSI, MISO, DELAY> ErrorType for BitbangSpiDevice<SCK, MOSI, MISO, DELAY>
where
    SCK: OutputPin,
    MOSI: OutputPin,
    MISO: InputPin,
    DELAY: DelayNs,
{
    type Error = core::convert::Infallible;
}

impl<SCK, MOSI, MISO, DELAY> SpiDevice<u8> for BitbangSpiDevice<SCK, MOSI, MISO, DELAY>
where
    SCK: OutputPin,
    MOSI: OutputPin,
    MISO: InputPin,
    DELAY: DelayNs,
{
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        for op in operations.iter_mut() {
            match op {
                Operation::Read(words) => {
                    for byte in words.iter_mut() {
                        *byte = self.transfer_byte(0xFF);
                    }
                }
                Operation::Write(words) => {
                    for &byte in words.iter() {
                        self.transfer_byte(byte);
                    }
                }
                Operation::Transfer(read, write) => {
                    for (r, w) in read.iter_mut().zip(write.iter()) {
                        *r = self.transfer_byte(*w);
                    }
                }
                Operation::TransferInPlace(words) => {
                    for byte in words.iter_mut() {
                        *byte = self.transfer_byte(*byte);
                    }
                }
                Operation::DelayNs(ns) => {
                    self.delay.delay_ns(*ns);
                }
            }
        }

        Ok(())
    }
}

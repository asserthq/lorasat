use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;

use super::error::Error;

const ADDR: u8 = 0x30;

const XOUT0: u8 = 0x00;
const TOUT: u8 = 0x07;
const STATUS: u8 = 0x08;
const CTRL0: u8 = 0x09;
const CTRL1: u8 = 0x0A;
const PRODUCT_ID: u8 = 0x2F;

const TM_M: u8 = 0x01;
const TM_T: u8 = 0x02;
const SET: u8 = 0x08;
const RESET: u8 = 0x10;
const OTP_READ: u8 = 0x40;

const SW_RST: u8 = 0x80;
const BW_200HZ: u8 = 0b10;

const MEAS_M_DONE: u8 = 0x01;
const MEAS_T_DONE: u8 = 0x02;

const EXPECTED_ID: u8 = 0x30;

const SENSITIVITY: f32 = 16384.0;

fn sign18(v: u32) -> i32 {
    (v & 0x3FFFF) as i32 - 0x20000
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MagneticField {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

impl MagneticField {
    pub const fn x_raw(&self) -> i32 {
        self.x
    }

    pub const fn y_raw(&self) -> i32 {
        self.y
    }

    pub const fn z_raw(&self) -> i32 {
        self.z
    }

    pub fn x_gauss(&self) -> f32 {
        self.x as f32 / SENSITIVITY
    }

    pub fn y_gauss(&self) -> f32 {
        self.y as f32 / SENSITIVITY
    }

    pub fn z_gauss(&self) -> f32 {
        self.z as f32 / SENSITIVITY
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Temperature {
    raw: u8,
}

impl Temperature {
    pub const fn raw(&self) -> u8 {
        self.raw
    }

    pub fn degrees_celsius(&self) -> f32 {
        -75.0 + self.raw as f32 * 0.8
    }
}

pub struct Mmc5983Ma<I2C, DLY> {
    i2c: I2C,
    delay: DLY,
}

impl<I2C, DLY, E> Mmc5983Ma<I2C, DLY>
where
    I2C: I2c<Error = E>,
    DLY: DelayNs,
{
    pub async fn new(i2c: I2C, delay: DLY) -> Result<Self, Error<E>> {
        let mut dev = Self { i2c, delay };
        dev.init().await?;
        Ok(dev)
    }

    pub fn destroy(self) -> I2C {
        self.i2c
    }

    async fn init(&mut self) -> Result<(), Error<E>> {
        let id = self.read_reg(PRODUCT_ID).await?;
        if id != EXPECTED_ID {
            return Err(Error::InvalidId(id));
        }

        self.write_reg(CTRL1, SW_RST).await?;
        self.delay.delay_ms(20).await;

        self.write_reg(CTRL0, OTP_READ).await?;
        self.delay.delay_ms(20).await;

        self.write_reg(CTRL1, BW_200HZ).await?;

        Ok(())
    }

    async fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), Error<E>> {
        self.i2c.write(ADDR, &[reg, val]).await.map_err(Error::Comm)
    }

    async fn read_reg(&mut self, reg: u8) -> Result<u8, Error<E>> {
        let mut buf = [0u8; 1];
        self.i2c
            .write_read(ADDR, &[reg], &mut buf)
            .await
            .map_err(Error::Comm)?;
        Ok(buf[0])
    }

    async fn read_consecutive(&mut self, start: u8, buf: &mut [u8]) -> Result<(), Error<E>> {
        self.i2c
            .write_read(ADDR, &[start], buf)
            .await
            .map_err(Error::Comm)
    }

    async fn wait_status(&mut self, bit: u8) -> Result<(), Error<E>> {
        for _ in 0..1000 {
            let st = self.read_reg(STATUS).await?;
            if st & bit != 0 {
                return Ok(());
            }
            self.delay.delay_ms(1).await;
        }
        Err(Error::Timeout)
    }

    async fn read_raw_field(&mut self) -> Result<MagneticField, Error<E>> {
        let mut buf = [0u8; 7];
        self.read_consecutive(XOUT0, &mut buf).await?;

        let x = ((buf[0] as u32) << 10) | ((buf[1] as u32) << 2) | ((buf[6] >> 6) & 0b11) as u32;
        let y = ((buf[2] as u32) << 10) | ((buf[3] as u32) << 2) | ((buf[6] >> 4) & 0b11) as u32;
        let z = ((buf[4] as u32) << 10) | ((buf[5] as u32) << 2) | ((buf[6] >> 2) & 0b11) as u32;

        Ok(MagneticField {
            x: sign18(x),
            y: sign18(y),
            z: sign18(z),
        })
    }

    async fn measure(&mut self) -> Result<MagneticField, Error<E>> {
        self.write_reg(CTRL0, TM_M).await?;
        self.wait_status(MEAS_M_DONE).await?;
        self.read_raw_field().await
    }

    pub async fn temperature(&mut self) -> Result<Temperature, Error<E>> {
        self.write_reg(CTRL0, TM_T).await?;
        self.wait_status(MEAS_T_DONE).await?;
        Ok(Temperature {
            raw: self.read_reg(TOUT).await?,
        })
    }

    async fn set_reset_measure(&mut self, ctrl: u8) -> Result<MagneticField, Error<E>> {
        self.write_reg(CTRL0, ctrl).await?;
        self.delay.delay_us(2).await;
        self.measure().await
    }

    pub async fn magnetic_field(&mut self) -> Result<MagneticField, Error<E>> {
        let f_set = self.set_reset_measure(SET).await?;
        let f_reset = self.set_reset_measure(RESET).await?;
        Ok(MagneticField {
            x: (f_set.x - f_reset.x) / 2,
            y: (f_set.y - f_reset.y) / 2,
            z: (f_set.z - f_reset.z) / 2,
        })
    }

    pub async fn raw_magnetic_field(&mut self) -> Result<MagneticField, Error<E>> {
        self.measure().await
    }

    pub async fn calibrate_offset(&mut self) -> Result<MagneticField, Error<E>> {
        let f_set = self.set_reset_measure(SET).await?;
        let f_reset = self.set_reset_measure(RESET).await?;
        Ok(MagneticField {
            x: (f_set.x + f_reset.x) / 2,
            y: (f_set.y + f_reset.y) / 2,
            z: (f_set.z + f_reset.z) / 2,
        })
    }
}

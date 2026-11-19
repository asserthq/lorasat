use embassy_embedded_hal::{adapter::BlockingAsync, shared_bus::asynch::i2c::I2cDevice};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use static_cell::StaticCell;

use crate::board::{BlockingI2cBus, I2cBus, I2cParts};

type I2c1Mutex = Mutex<CriticalSectionRawMutex, I2cBus>;
type I2c2Bus = BlockingAsync<BlockingI2cBus>;
type I2c2Mutex = Mutex<CriticalSectionRawMutex, I2c2Bus>;

pub(crate) type SharedI2cDevice = I2cDevice<'static, CriticalSectionRawMutex, I2cBus>;
pub(crate) type SharedBlockingI2cDevice = I2cDevice<'static, CriticalSectionRawMutex, I2c2Bus>;

static I2C1: StaticCell<I2c1Mutex> = StaticCell::new();
static I2C2: StaticCell<I2c2Mutex> = StaticCell::new();

pub(crate) struct SharedI2cBuses {
    bus1: &'static I2c1Mutex,
    bus2: &'static I2c2Mutex,
    pub bus3: BlockingI2cBus,
}

impl SharedI2cBuses {
    pub(crate) fn init(parts: I2cParts) -> Self {
        let bus1 = I2C1.init(Mutex::new(parts.bus1));
        let bus2 = I2C2.init(Mutex::new(BlockingAsync::new(parts.bus2)));

        Self {
            bus1,
            bus2,
            bus3: parts.bus3,
        }
    }

    pub(crate) fn bus1(&self) -> SharedI2cDevice {
        I2cDevice::new(self.bus1)
    }

    /// I2C2 has no DMA stream available. Its async API wraps blocking
    /// transactions, so accesses must remain short.
    pub(crate) fn bus2(&self) -> SharedBlockingI2cDevice {
        I2cDevice::new(self.bus2)
    }
}

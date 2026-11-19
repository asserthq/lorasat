use embassy_stm32::{
    Peri, bind_interrupts, dma,
    i2c::{self, I2c},
    mode::{Async, Blocking},
    peripherals,
    time::Hertz,
};

bind_interrupts!(pub(crate) struct I2cIrqs {
    I2C1_EV => i2c::EventInterruptHandler<peripherals::I2C1>;
    I2C1_ER => i2c::ErrorInterruptHandler<peripherals::I2C1>;

    // I2C1 — DMA.
    DMA1_STREAM0 => dma::InterruptHandler<peripherals::DMA1_CH0>;
    DMA1_STREAM6 => dma::InterruptHandler<peripherals::DMA1_CH6>;
});

pub(crate) type I2cBus = I2c<'static, Async, i2c::mode::Master>;
pub(crate) type BlockingI2cBus = I2c<'static, Blocking, i2c::mode::Master>;

pub(crate) struct I2cParts {
    pub bus1: I2cBus,
    pub bus2: BlockingI2cBus,
    pub bus3: BlockingI2cBus,
}

pub(crate) struct I2cHardware {
    pub i2c1: Peri<'static, peripherals::I2C1>,
    pub i2c1_scl: Peri<'static, peripherals::PB8>,
    pub i2c1_sda: Peri<'static, peripherals::PB9>,
    pub i2c1_tx_dma: Peri<'static, peripherals::DMA1_CH6>,
    pub i2c1_rx_dma: Peri<'static, peripherals::DMA1_CH0>,

    pub i2c2: Peri<'static, peripherals::I2C2>,
    pub i2c2_scl: Peri<'static, peripherals::PB10>,
    pub i2c2_sda: Peri<'static, peripherals::PB11>,

    pub i2c3: Peri<'static, peripherals::I2C3>,
    pub i2c3_scl: Peri<'static, peripherals::PA8>,
    pub i2c3_sda: Peri<'static, peripherals::PC9>,
}

pub(crate) fn init(hw: I2cHardware) -> I2cParts {
    let mut config = i2c::Config::default();
    config.frequency = Hertz(400_000);
    config.sda_pullup = false;
    config.scl_pullup = false;

    let bus1 = I2c::new(
        hw.i2c1,
        hw.i2c1_scl,
        hw.i2c1_sda,
        hw.i2c1_tx_dma,
        hw.i2c1_rx_dma,
        I2cIrqs,
        config,
    );

    let bus2 = I2c::new_blocking(hw.i2c2, hw.i2c2_scl, hw.i2c2_sda, config);

    let bus3 = I2c::new_blocking(hw.i2c3, hw.i2c3_scl, hw.i2c3_sda, config);

    I2cParts { bus1, bus2, bus3 }
}

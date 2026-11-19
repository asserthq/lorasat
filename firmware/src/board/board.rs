use embassy_stm32::{Config, Peripherals, rcc::Sysclk};

use super::{
    coils::{self, CoilsHardware, CoilsParts},
    flash::{self, FlashHardware, FlashParts},
    i2c::{self, I2cHardware, I2cParts},
    radio::{self, PlatformRadioHardware, RadioParts, ServiceRadioHardware},
};

pub(crate) struct BoardParts {
    pub i2c: I2cParts,
    pub radios: RadioParts,
    pub flash: FlashParts,
    pub coils: CoilsParts,
}

pub(crate) fn init() -> BoardParts {
    let mut config = Config::default();
    config.rcc.hsi = true;
    config.rcc.sys = Sysclk::HSI;

    split(embassy_stm32::init(config))
}

/// Applies this board's pin mapping and splits MCU peripherals by device.
fn split(p: Peripherals) -> BoardParts {
    let i2c = i2c::init(I2cHardware {
        i2c1: p.I2C1,
        i2c1_scl: p.PB8,
        i2c1_sda: p.PB9,
        i2c1_tx_dma: p.DMA1_CH6,
        i2c1_rx_dma: p.DMA1_CH0,
        i2c2: p.I2C2,
        i2c2_scl: p.PB10,
        i2c2_sda: p.PB11,
        i2c3: p.I2C3,
        i2c3_scl: p.PA8,
        i2c3_sda: p.PC9,
    });

    let coils = coils::init(CoilsHardware {
        tim3: p.TIM3,
        tim4: p.TIM4,
        y_in1: p.PC6,
        y_in2: p.PC7,
        z_in1: p.PB1,
        z_in2: p.PB0,
        x_in1: p.PD14,
        x_in2: p.PD15,
        sleep: p.PC5,
    });

    let radios = radio::init(
        PlatformRadioHardware {
            spi: p.SPI2,
            sck: p.PB13,
            mosi: p.PB15,
            miso: p.PB14,
            tx_dma: p.DMA1_CH4,
            rx_dma: p.DMA1_CH3,
            nss: p.PB12,
            reset: p.PD3,
            dio1: p.PD4,
            dio1_exti: p.EXTI4,
            busy: p.PD5,
            busy_exti: p.EXTI5,
        },
        ServiceRadioHardware {
            spi: p.SPI3,
            sck: p.PC10,
            mosi: p.PC12,
            miso: p.PC11,

            tx_dma: p.DMA1_CH5,
            rx_dma: p.DMA1_CH2,
            nss: p.PA15,
            reset: p.PD0,
            dio1: p.PD2,
            dio1_exti: p.EXTI2,
            busy: p.PD1,
            busy_exti: p.EXTI1,
        },
    );

    let flash = flash::init(FlashHardware {
        spi: p.SPI1,
        sck: p.PA5,
        mosi: p.PA7,
        miso: p.PA6,
        tx_dma: p.DMA2_CH3,
        rx_dma: p.DMA2_CH0,
        nss: p.PA4,
    });

    BoardParts {
        i2c,
        radios,
        flash,
        coils,
    }
}

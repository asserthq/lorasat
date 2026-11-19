use embassy_stm32::{
    Peri,
    gpio::{Level, Output, OutputType, Speed},
    peripherals,
    time::Hertz,
    timer::{
        low_level::CountingMode,
        simple_pwm::{PwmPin, SimplePwm, SimplePwmChannel},
    },
};

use crate::config;

const PWM_FREQUENCY: Hertz = Hertz(config::coils::PWM_FREQUENCY_HZ);

pub(crate) type Tim3PwmChannel = SimplePwmChannel<'static, peripherals::TIM3>;
pub(crate) type Tim4PwmChannel = SimplePwmChannel<'static, peripherals::TIM4>;

pub(crate) struct CoilsParts {
    // X: PD14 = TIM4_CH3, PD15 = TIM4_CH4.
    pub x_in1: Tim4PwmChannel,
    pub x_in2: Tim4PwmChannel,
    // Y: PC6 = TIM3_CH1, PC7 = TIM3_CH2.
    pub y_in1: Tim3PwmChannel,
    pub y_in2: Tim3PwmChannel,
    // Z: PB1 = TIM3_CH4, PB0 = TIM3_CH3.
    pub z_in1: Tim3PwmChannel,
    pub z_in2: Tim3PwmChannel,
    pub sleep: Output<'static>,
}

pub(crate) struct CoilsHardware {
    pub tim3: Peri<'static, peripherals::TIM3>,
    pub tim4: Peri<'static, peripherals::TIM4>,

    pub x_in1: Peri<'static, peripherals::PD14>,
    pub x_in2: Peri<'static, peripherals::PD15>,
    pub y_in1: Peri<'static, peripherals::PC6>,
    pub y_in2: Peri<'static, peripherals::PC7>,
    pub z_in1: Peri<'static, peripherals::PB1>,
    pub z_in2: Peri<'static, peripherals::PB0>,

    pub sleep: Peri<'static, peripherals::PC5>,
}

pub(crate) fn init(hw: CoilsHardware) -> CoilsParts {
    let CoilsHardware {
        tim3,
        tim4,
        x_in1,
        x_in2,
        y_in1,
        y_in2,
        z_in1,
        z_in2,
        sleep,
    } = hw;

    let pwm_yz = SimplePwm::new(
        tim3,
        Some(PwmPin::new(y_in1, OutputType::PushPull)),
        Some(PwmPin::new(y_in2, OutputType::PushPull)),
        Some(PwmPin::new(z_in2, OutputType::PushPull)),
        Some(PwmPin::new(z_in1, OutputType::PushPull)),
        PWM_FREQUENCY,
        CountingMode::EdgeAlignedUp,
    );

    let pwm_x = SimplePwm::new(
        tim4,
        None,
        None,
        Some(PwmPin::new(x_in1, OutputType::PushPull)),
        Some(PwmPin::new(x_in2, OutputType::PushPull)),
        PWM_FREQUENCY,
        CountingMode::EdgeAlignedUp,
    );

    let mut yz = pwm_yz.split();
    let mut x = pwm_x.split();

    yz.ch1.set_duty_cycle_fully_off();
    yz.ch2.set_duty_cycle_fully_off();
    yz.ch3.set_duty_cycle_fully_off();
    yz.ch4.set_duty_cycle_fully_off();
    x.ch3.set_duty_cycle_fully_off();
    x.ch4.set_duty_cycle_fully_off();

    yz.ch1.enable();
    yz.ch2.enable();
    yz.ch3.enable();
    yz.ch4.enable();
    x.ch3.enable();
    x.ch4.enable();

    let sleep = Output::new(sleep, Level::High, Speed::Low);

    CoilsParts {
        x_in1: x.ch3,
        x_in2: x.ch4,
        y_in1: yz.ch1,
        y_in2: yz.ch2,
        z_in1: yz.ch4,
        z_in2: yz.ch3,
        sleep,
    }
}

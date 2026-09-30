#![no_std]
#![no_main]

use defmt::info;

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, OutputType, Speed};
use embassy_stm32::time::khz;
use embassy_stm32::timer::GeneralInstance4Channel;
use embassy_stm32::timer::low_level::CountingMode;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm, SimplePwmChannel};
use embassy_time::{Duration, Timer};

use sandbox_lib as _;

use sat_drivers::drv8833::{Drv8833, PwmPin as MotorPwmPin};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    info!("drv8833 self-test start");

    let in1 = PwmPin::new(p.PA0, OutputType::PushPull);
    let in2 = PwmPin::new(p.PA1, OutputType::PushPull);

    let pwm = SimplePwm::new(
        p.TIM2,
        Some(in1),
        Some(in2),
        None,
        None,
        khz(30),
        CountingMode::EdgeAlignedUp,
    );

    let mut channels = pwm.split();
    channels.ch1.enable();
    channels.ch2.enable();

    let sleep = Output::new(p.PB0, Level::High, Speed::Low);

    let mut motor = Drv8833::new(Channel(channels.ch1), Channel(channels.ch2), Some(sleep));
    motor.init();
    info!("drv8833: init done");

    motor.set_speed(100);
    info!("drv8833: forward 100%");
    Timer::after(Duration::from_secs(2)).await;

    motor.set_speed(0);
    info!("drv8833: stop");
    Timer::after(Duration::from_millis(500)).await;

    motor.set_speed(-100);
    info!("drv8833: reverse 100%");
    Timer::after(Duration::from_secs(2)).await;

    motor.set_speed(0);
    motor.sleep();
    info!("drv8833: done");
}

struct Channel<T>(SimplePwmChannel<'static, T>)
where
    T: GeneralInstance4Channel;

impl<T: GeneralInstance4Channel> MotorPwmPin for Channel<T> {
    fn set_duty(&mut self, duty: u16) {
        let percent = (duty as u32 * 100 / u16::MAX as u32) as u8;
        self.0.set_duty_cycle_percent(percent);
    }
}

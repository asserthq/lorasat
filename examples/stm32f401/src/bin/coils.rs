#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Input, Output, OutputType};
use embassy_stm32::time::khz;
use embassy_stm32::timer::low_level::CountingMode;
use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
use embassy_time::Timer;
use nalgebra::Vector3;
use panic_probe as _;
use sat_core::adcs::Coils;
use sat_drivers::coils::CoilsImpl;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    let in1_x = PwmPin::new(p.PA0, OutputType::PushPull);
    let in2_x = PwmPin::new(p.PA1, OutputType::PushPull);
    let in1_y = PwmPin::new(p.PA2, OutputType::PushPull);
    let in2_y = PwmPin::new(p.PA3, OutputType::PushPull);

    let in1_z = PwmPin::new(p.PA8, OutputType::PushPull);
    let in2_z = PwmPin::new(p.PA9, OutputType::PushPull);

    let pwm_xy = SimplePwm::new(
        p.TIM5,
        Some(in1_x),
        Some(in2_x),
        Some(in1_y),
        Some(in2_y),
        khz(20),
        CountingMode::EdgeAlignedUp,
    );

    let pwm_z = SimplePwm::new(
        p.TIM1,
        Some(in1_z),
        Some(in2_z),
        None,
        None,
        khz(20),
        CountingMode::EdgeAlignedUp,
    );

    let mut channels_xy = pwm_xy.split();
    let mut channels_z = pwm_z.split();

    channels_xy.ch1.enable();
    channels_xy.ch2.enable();
    channels_xy.ch3.enable();
    channels_xy.ch4.enable();
    channels_z.ch1.enable();
    channels_z.ch2.enable();

    let (x1, x2, y1, y2) = (
        channels_xy.ch1,
        channels_xy.ch2,
        channels_xy.ch3,
        channels_xy.ch4,
    );
    let (z1, z2) = (channels_z.ch1, channels_z.ch2);

    let mut coils = CoilsImpl::new(
        x1,
        x2,
        y1,
        y2,
        z1,
        z2,
        None::<Output<'static>>,
        None::<Input<'static>>,
        None::<Input<'static>>,
    );

    loop {
        info!("applying (1, 1, 1)");
        coils
            .apply_pwm(Vector3::new(1.0, 1.0, 1.0))
            .await
            .unwrap();
        Timer::after_secs(5).await;

        info!("applying (0, 0, 0)");
        coils
            .apply_pwm(Vector3::new(0.0, 0.0, 0.0))
            .await
            .unwrap();
        Timer::after_secs(5).await;

        info!("applying (-1, -1, -1)");
        coils
            .apply_pwm(Vector3::new(-1.0, -1.0, -1.0))
            .await
            .unwrap();
        Timer::after_secs(5).await;

        info!("raise from (-1, -1, -1) to (1, 1, 1) within 5 sec");
        let steps = 100;
        let time_secs = 5;
        let step = 2.0 / steps as f32;
        let delay_ms = (time_secs * 1000) / steps;

        for i in 1..=steps {
            let level = -1.0 + step * i as f32;
            coils
                .apply_pwm(Vector3::new(level, level, level))
                .await
                .unwrap();
            Timer::after_millis(delay_ms).await;
        }

        info!("applying (0, 0, 0)");
        coils
            .apply_pwm(Vector3::new(0.0, 0.0, 0.0))
            .await
            .unwrap();
        Timer::after_secs(5).await;
    }
}

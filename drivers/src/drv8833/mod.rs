use embedded_hal::digital::OutputPin;

pub trait PwmPin {
    fn set_duty(&mut self, duty: u16);
}

pub struct Drv8833<IN1, IN2, SLEEP> {
    in1: IN1,
    in2: IN2,
    sleep: Option<SLEEP>,
}

impl<IN1, IN2, SLEEP> Drv8833<IN1, IN2, SLEEP>
where
    IN1: PwmPin,
    IN2: PwmPin,
    SLEEP: OutputPin,
{
    pub fn new(in1: IN1, in2: IN2, sleep: Option<SLEEP>) -> Self {
        Self { in1, in2, sleep }
    }

    pub fn init(&mut self) {
        if let Some(sleep) = &mut self.sleep {
            let _ = sleep.set_high();
        }
        self.in1.set_duty(0);
        self.in2.set_duty(0);
    }

    pub fn set_speed(&mut self, speed: i8) {
        let speed = speed.clamp(-100, 100);

        if speed > 0 {
            let duty = percent_to_duty(speed as u16);
            self.in1.set_duty(duty);
            self.in2.set_duty(0);
        } else if speed < 0 {
            let duty = percent_to_duty((-speed) as u16);
            self.in1.set_duty(0);
            self.in2.set_duty(duty);
        } else {
            self.in1.set_duty(0);
            self.in2.set_duty(0);
        }
    }

    pub fn sleep(&mut self) {
        if let Some(sleep) = &mut self.sleep {
            let _ = sleep.set_low();
        }
    }
}

pub fn percent_to_duty(percent: u16) -> u16 {
    let percent = percent.min(100);
    ((percent as u32 * u16::MAX as u32) / 100) as u16
}

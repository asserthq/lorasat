#![cfg(not(feature = "async"))]

use core::convert::Infallible;
use embedded_hal::{
    delay::DelayNs,
    i2c::{ErrorType, I2c, Operation},
};
use mmc5983_rs::{register_address::RegRead, Error, MagneticField, Mmc5983};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

#[derive(Debug)]
enum Step {
    Write(Vec<u8>),
    Read(u8, Vec<u8>),
    Delay(u32),
}

type Steps = Rc<RefCell<VecDeque<Step>>>;

struct Bus(Steps);
struct Delay(Steps);

impl ErrorType for Bus {
    type Error = Infallible;
}

impl I2c for Bus {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Infallible> {
        assert_eq!(address, 0x30);
        let step = self
            .0
            .borrow_mut()
            .pop_front()
            .expect("unexpected I2C access");
        match (step, operations) {
            (Step::Write(expected), [Operation::Write(bytes)]) => assert_eq!(*bytes, expected),
            (Step::Read(register, data), [Operation::Write(bytes), Operation::Read(buffer)]) => {
                assert_eq!(*bytes, [register]);
                buffer.copy_from_slice(&data);
            }
            (step, _) => panic!("unexpected transaction, expected {step:?}"),
        }
        Ok(())
    }
}

impl DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        match self.0.borrow_mut().pop_front().expect("unexpected delay") {
            Step::Delay(expected) => assert_eq!(ns, expected),
            step => panic!("expected {step:?}, got delay of {ns} ns"),
        }
    }
}

#[test]
fn initialization_waits_after_reset_and_otp_and_does_not_cache_otp_command() {
    let steps: Steps = Rc::new(RefCell::new(VecDeque::from([
        Step::Read(0x2F, vec![0x30]),
        Step::Write(vec![0x0A, 0x80]),
        Step::Delay(20_000_000),
        Step::Write(vec![0x09, 0x40]),
        Step::Delay(20_000_000),
        Step::Write(vec![0x09, 0x04]),
        Step::Write(vec![0x0A, 0x00]),
        Step::Write(vec![0x09, 0x0C]),
        Step::Delay(1_000),
        Step::Delay(1_000_000),
        Step::Write(vec![0x09, 0x24]),
        Step::Write(vec![0x08, 0x01]),
        Step::Write(vec![0x09, 0x25]),
    ])));
    let mut sensor = Mmc5983::new_with_i2c(Bus(steps.clone()));
    sensor.init_with_delay(&mut Delay(steps.clone())).unwrap();
    assert!(matches!(
        sensor.magnetic_field(),
        Err(nb::Error::WouldBlock)
    ));
    assert!(steps.borrow().is_empty());
}

#[test]
fn polling_starts_once_and_next_call_starts_a_fresh_measurement() {
    let steps: Steps = Rc::new(RefCell::new(VecDeque::from([
        Step::Write(vec![0x08, 0x01]),
        Step::Write(vec![0x09, 0x01]),
        Step::Read(0x08, vec![0x00]),
        Step::Read(0x08, vec![0x01]),
        Step::Read(0x00, vec![0x80, 0x00, 0x90, 0x00, 0x70, 0x00, 0x00]),
        Step::Write(vec![0x08, 0x01]),
        Step::Write(vec![0x09, 0x01]),
    ])));
    let mut sensor = Mmc5983::new_with_i2c(Bus(steps.clone()));
    assert!(matches!(
        sensor.magnetic_field(),
        Err(nb::Error::WouldBlock)
    ));
    assert!(matches!(
        sensor.magnetic_field(),
        Err(nb::Error::WouldBlock)
    ));
    assert_eq!(sensor.magnetic_field().unwrap().gauss(), (0.0, 1.0, -1.0));
    assert!(matches!(
        sensor.magnetic_field(),
        Err(nb::Error::WouldBlock)
    ));
    assert!(steps.borrow().is_empty());
}

#[test]
fn incorrect_product_id_stops_before_reset() {
    let steps: Steps = Rc::new(RefCell::new(VecDeque::from([Step::Read(0x2F, vec![0x00])])));
    let mut sensor = Mmc5983::new_with_i2c(Bus(steps.clone()));
    assert!(matches!(
        sensor.init_with_delay(&mut Delay(steps.clone())),
        Err(Error::InvalidId(_))
    ));
    assert!(steps.borrow().is_empty());
}

#[test]
fn calibrated_field_preserves_signed_values_without_unsigned_underflow() {
    let steps: Steps = Rc::new(RefCell::new(VecDeque::from([
        Step::Write(vec![0x09, 0x01]),
        Step::Read(0x08, vec![0x01]),
        Step::Read(0x00, vec![0x80, 0x00, 0x90, 0x00, 0x70, 0x00, 0x00]),
    ])));
    let mut sensor = Mmc5983::new_with_i2c(Bus(steps.clone()));
    assert_eq!(
        sensor.get_calibrated_field().unwrap().gauss(),
        (0.0, 1.0, -1.0)
    );
    assert!(steps.borrow().is_empty());
}

#[test]
fn gauss_conversion_preserves_zero_sign_and_eighteen_bit_limits() {
    let field = MagneticField::from_data((0, 131_072, 262_143));
    assert_eq!(field.gauss(), (-8.0, 0.0, 8.0 - 1.0 / 16_384.0));
}

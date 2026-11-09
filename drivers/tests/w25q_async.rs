use std::{
    cell::RefCell,
    collections::BTreeMap,
    future::{Future, poll_fn},
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

use embedded_hal_async::{
    delay::DelayNs,
    spi::{ErrorType, Operation, SpiDevice},
};
use embedded_storage_async::nor_flash::{
    MultiwriteNorFlash, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};
use sat_drivers::flash::asynchronous::{AsyncFlashError, AsyncW25Q64};

const CAPACITY: u32 = 8 * 1024 * 1024;
const SECTOR_SIZE: u32 = 4096;

fn run_ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..100_000 {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
    panic!("test future did not complete within its poll budget");
}

async fn yield_once() {
    let mut yielded = false;
    poll_fn(|context| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SpiFault(u8);

impl embedded_hal_async::spi::Error for SpiFault {
    fn kind(&self) -> embedded_hal_async::spi::ErrorKind {
        embedded_hal_async::spi::ErrorKind::Other
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Status(u8),
    Jedec,
    Read(u32, usize),
    WriteEnable,
    Program(u32, Vec<u8>),
    Erase(u32),
}

struct State {
    id: [u8; 3],
    memory: BTreeMap<u32, u8>,
    events: Vec<Event>,
    delays: Vec<u32>,
    wel: bool,
    deny_wel: bool,
    busy_polls: usize,
    busy_after_command: usize,
    stuck_busy: bool,
    fail_on: Option<u8>,
    yield_spi: bool,
    yield_delay: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            id: [0xEF, 0x40, 0x17],
            memory: BTreeMap::new(),
            events: Vec::new(),
            delays: Vec::new(),
            wel: false,
            deny_wel: false,
            busy_polls: 0,
            busy_after_command: 1,
            stuck_busy: false,
            fail_on: None,
            yield_spi: false,
            yield_delay: false,
        }
    }
}

type Shared = Rc<RefCell<State>>;

struct MockSpi(Shared);
struct MockDelay(Shared);
type Flash = AsyncW25Q64<MockSpi, MockDelay>;

fn peripherals() -> (MockSpi, MockDelay, Shared) {
    let state = Rc::new(RefCell::new(State::default()));
    (MockSpi(state.clone()), MockDelay(state.clone()), state)
}

fn initialized() -> (Flash, Shared) {
    let (spi, delay, state) = peripherals();
    let flash = run_ready(Flash::new(spi, delay)).unwrap();
    state.borrow_mut().events.clear();
    (flash, state)
}

fn address(header: &[u8]) -> u32 {
    assert_eq!(header.len(), 4);
    u32::from_be_bytes([0, header[1], header[2], header[3]])
}

impl ErrorType for MockSpi {
    type Error = SpiFault;
}

impl SpiDevice<u8> for MockSpi {
    async fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), SpiFault> {
        let should_yield = self.0.borrow().yield_spi;
        if should_yield {
            yield_once().await;
        }
        let header = match &operations[0] {
            Operation::Write(bytes) => bytes.to_vec(),
            _ => panic!("expected a command write at the start of a transaction"),
        };
        let command = header[0];
        let mut state = self.0.borrow_mut();
        if state.fail_on == Some(command) {
            state.fail_on = None;
            return Err(SpiFault(command));
        }
        if command != 0x05 {
            assert!(
                !state.stuck_busy && state.busy_polls == 0,
                "access while busy"
            );
        }
        match command {
            0x05 => {
                assert_eq!(header, [0x05]);
                assert_eq!(operations.len(), 2);
                let busy = state.stuck_busy || state.busy_polls > 0;
                state.busy_polls = state.busy_polls.saturating_sub(1);
                let status = u8::from(busy) | if state.wel { 2 } else { 0 };
                let Operation::Read(output) = &mut operations[1] else {
                    panic!("expected a status read");
                };
                assert_eq!(output.len(), 1);
                output[0] = status;
                state.events.push(Event::Status(status));
            }
            0x9F => {
                assert_eq!(header, [0x9F]);
                assert_eq!(operations.len(), 2);
                let Operation::Read(output) = &mut operations[1] else {
                    panic!("expected a JEDEC read");
                };
                output.copy_from_slice(&state.id);
                state.events.push(Event::Jedec);
            }
            0x03 => {
                let offset = address(&header);
                assert_eq!(operations.len(), 2);
                let Operation::Read(output) = &mut operations[1] else {
                    panic!("expected a data read");
                };
                assert!(offset as usize + output.len() <= CAPACITY as usize);
                for (index, byte) in output.iter_mut().enumerate() {
                    *byte = state
                        .memory
                        .get(&(offset + index as u32))
                        .copied()
                        .unwrap_or(0xFF);
                }
                state.events.push(Event::Read(offset, output.len()));
            }
            0x06 => {
                assert_eq!(header, [0x06]);
                assert_eq!(operations.len(), 1);
                state.wel = !state.deny_wel;
                state.events.push(Event::WriteEnable);
            }
            0x02 => {
                let offset = address(&header);
                assert_eq!(operations.len(), 2);
                let Operation::Write(data) = &operations[1] else {
                    panic!("expected page data under the same chip select");
                };
                assert!(state.wel, "program without WEL");
                assert!(!data.is_empty());
                assert!(
                    offset % 256 + data.len() as u32 <= 256,
                    "program crossed a page"
                );
                assert!(offset as usize + data.len() <= CAPACITY as usize);
                for (index, byte) in data.iter().enumerate() {
                    *state.memory.entry(offset + index as u32).or_insert(0xFF) &= *byte;
                }
                state.events.push(Event::Program(offset, data.to_vec()));
                state.wel = false;
                state.busy_polls = state.busy_after_command;
            }
            0x20 => {
                let offset = address(&header);
                assert_eq!(operations.len(), 1);
                assert!(state.wel, "erase without WEL");
                assert_eq!(offset % SECTOR_SIZE, 0);
                assert!(offset + SECTOR_SIZE <= CAPACITY);
                state
                    .memory
                    .retain(|address, _| *address < offset || *address >= offset + SECTOR_SIZE);
                state.events.push(Event::Erase(offset));
                state.wel = false;
                state.busy_polls = state.busy_after_command;
            }
            _ => panic!("unexpected command {command:#04x}; init must not reset/erase/unprotect"),
        }
        Ok(())
    }
}

impl DelayNs for MockDelay {
    async fn delay_ns(&mut self, ns: u32) {
        let should_yield = {
            let mut state = self.0.borrow_mut();
            state.delays.push(ns);
            state.yield_delay
        };
        if should_yield {
            yield_once().await;
        }
    }
}

#[test]
fn accepts_both_models_and_initialization_only_waits_and_identifies() {
    for id in [[0xEF, 0x40, 0x17], [0xEF, 0x60, 0x17]] {
        let (spi, delay, state) = peripherals();
        {
            let mut state = state.borrow_mut();
            state.id = id;
            state.busy_polls = 2;
        }
        let mut flash = run_ready(Flash::new(spi, delay)).unwrap();
        assert_eq!(
            state.borrow().events,
            [
                Event::Status(1),
                Event::Status(1),
                Event::Status(0),
                Event::Jedec
            ]
        );
        assert_eq!(state.borrow().delays, [100_000, 100_000]);
        assert_eq!(run_ready(flash.read_jedec_id()), Ok(id));
        let (spi, delay) = flash.into_inner();
        assert!(Rc::ptr_eq(&spi.0, &state));
        assert!(Rc::ptr_eq(&delay.0, &state));
    }
}

#[test]
fn rejects_unsupported_jedec_ids_without_mutating_the_chip() {
    for id in [
        [0; 3],
        [0xFF; 3],
        [0xEF, 0x40, 0x18],
        [0xEF, 0x60, 0x16],
        [0x20, 0x40, 0x17],
    ] {
        let (spi, delay, state) = peripherals();
        state.borrow_mut().id = id;
        assert!(matches!(
            run_ready(Flash::new(spi, delay)),
            Err(AsyncFlashError::UnsupportedDevice(actual)) if actual == id
        ));
        assert_eq!(state.borrow().events, [Event::Status(0), Event::Jedec]);
    }
}

#[test]
fn implements_standard_traits_and_error_kinds() {
    fn require_multiwrite<T: MultiwriteNorFlash>() {}
    require_multiwrite::<Flash>();
    let (flash, _) = initialized();
    assert_eq!(flash.capacity(), CAPACITY as usize);
    assert_eq!(<Flash as ReadNorFlash>::READ_SIZE, 1);
    assert_eq!(<Flash as NorFlash>::WRITE_SIZE, 1);
    assert_eq!(<Flash as NorFlash>::ERASE_SIZE, 4096);
    assert_eq!(
        AsyncFlashError::<SpiFault>::OutOfBounds.kind(),
        NorFlashErrorKind::OutOfBounds
    );
    assert_eq!(
        AsyncFlashError::<SpiFault>::NotAligned.kind(),
        NorFlashErrorKind::NotAligned
    );
    for error in [
        AsyncFlashError::Spi(SpiFault(3)),
        AsyncFlashError::Timeout,
        AsyncFlashError::WriteDisabled,
        AsyncFlashError::UnsupportedDevice([0; 3]),
    ] {
        assert_eq!(error.kind(), NorFlashErrorKind::Other);
    }
}

#[test]
fn rejects_entire_invalid_read_and_write_ranges_before_io() {
    let (mut flash, state) = initialized();
    for (offset, length) in [
        (CAPACITY, 1),
        (CAPACITY + 1, 0),
        (CAPACITY - 1, 2),
        (CAPACITY - 256, 257),
        (u32::MAX, 0),
        (u32::MAX, 2),
    ] {
        let mut bytes = vec![0x55; length];
        assert_eq!(
            run_ready(flash.read(offset, &mut bytes)),
            Err(AsyncFlashError::OutOfBounds)
        );
        assert_eq!(
            run_ready(flash.write(offset, &bytes)),
            Err(AsyncFlashError::OutOfBounds)
        );
        assert!(state.borrow().events.is_empty());
        assert!(state.borrow().delays.is_empty());
    }
}

#[test]
fn rejects_reversed_out_of_bounds_and_unaligned_erase_before_io() {
    let (mut flash, state) = initialized();
    for (from, to, expected) in [
        (SECTOR_SIZE, 0, AsyncFlashError::OutOfBounds),
        (1, 0, AsyncFlashError::OutOfBounds),
        (0, CAPACITY + SECTOR_SIZE, AsyncFlashError::OutOfBounds),
        (
            CAPACITY + SECTOR_SIZE,
            CAPACITY + SECTOR_SIZE,
            AsyncFlashError::OutOfBounds,
        ),
        (0, u32::MAX, AsyncFlashError::OutOfBounds),
        (1, SECTOR_SIZE, AsyncFlashError::NotAligned),
        (0, SECTOR_SIZE - 1, AsyncFlashError::NotAligned),
        (1, 1, AsyncFlashError::NotAligned),
    ] {
        assert_eq!(run_ready(flash.erase(from, to)), Err(expected));
        assert!(state.borrow().events.is_empty());
        assert!(state.borrow().delays.is_empty());
    }
}

#[test]
fn valid_empty_ranges_only_check_readiness_and_allow_capacity_offset() {
    let (mut flash, state) = initialized();
    for offset in [0, SECTOR_SIZE, CAPACITY] {
        assert_eq!(run_ready(flash.read(offset, &mut [])), Ok(()));
        assert_eq!(run_ready(flash.write(offset, &[])), Ok(()));
        assert_eq!(run_ready(flash.erase(offset, offset)), Ok(()));
    }
    assert_eq!(
        state.borrow().events,
        (0..9).map(|_| Event::Status(0)).collect::<Vec<_>>()
    );
    assert!(state.borrow().delays.is_empty());
}

#[test]
fn first_and_last_bytes_are_accessible_and_last_sector_can_be_erased() {
    let (mut flash, state) = initialized();
    for offset in [0, CAPACITY - 1] {
        run_ready(flash.write(offset, &[0x53])).unwrap();
        let mut output = [0];
        run_ready(flash.read(offset, &mut output)).unwrap();
        assert_eq!(output, [0x53]);
    }
    run_ready(flash.erase(CAPACITY - SECTOR_SIZE, CAPACITY)).unwrap();
    let mut output = [0];
    run_ready(flash.read(CAPACITY - 1, &mut output)).unwrap();
    assert_eq!(output, [0xFF]);
    assert_eq!(state.borrow().memory.get(&0), Some(&0x53));
}

#[test]
fn splits_cross_page_writes_and_enables_and_polls_each_program() {
    let (mut flash, state) = initialized();
    let data: Vec<_> = (0..520).map(|index| index as u8).collect();
    run_ready(flash.write(250, &data)).unwrap();
    let mut expected = vec![Event::Status(0)];
    let mut written = 0;
    for (offset, length) in [(250, 6), (256, 256), (512, 256), (768, 2)] {
        expected.extend([
            Event::WriteEnable,
            Event::Status(2),
            Event::Program(offset, data[written..written + length].to_vec()),
            Event::Status(1),
            Event::Status(0),
        ]);
        written += length;
    }
    assert_eq!(state.borrow().events, expected);
    assert_eq!(state.borrow().delays, [100_000; 4]);
    let mut output = vec![0; data.len()];
    run_ready(flash.read(250, &mut output)).unwrap();
    assert_eq!(output, data);
}

#[test]
fn repeated_programming_is_bitwise_and_without_implicit_erase() {
    let (mut flash, state) = initialized();
    run_ready(flash.write(255, &[0xAA, 0x55])).unwrap();
    run_ready(flash.write(255, &[0x0F, 0xF0])).unwrap();
    run_ready(flash.write(255, &[0xFF, 0xFF])).unwrap();
    let mut output = [0; 2];
    run_ready(flash.read(255, &mut output)).unwrap();
    assert_eq!(output, [0x0A, 0x50]);
    assert!(
        !state
            .borrow()
            .events
            .iter()
            .any(|event| matches!(event, Event::Erase(_)))
    );
}

#[test]
fn erases_each_sector_in_the_half_open_range_with_independent_wren_and_poll() {
    let (mut flash, state) = initialized();
    for offset in [
        0,
        SECTOR_SIZE - 1,
        SECTOR_SIZE,
        SECTOR_SIZE * 2 - 1,
        SECTOR_SIZE * 2,
    ] {
        state.borrow_mut().memory.insert(offset, 0);
    }
    run_ready(flash.erase(0, SECTOR_SIZE * 2)).unwrap();
    assert_eq!(
        state.borrow().events,
        [
            Event::Status(0),
            Event::WriteEnable,
            Event::Status(2),
            Event::Erase(0),
            Event::Status(1),
            Event::Status(0),
            Event::WriteEnable,
            Event::Status(2),
            Event::Erase(SECTOR_SIZE),
            Event::Status(1),
            Event::Status(0),
        ]
    );
    assert_eq!(state.borrow().delays, [100_000; 2]);
    assert_eq!(
        state.borrow().memory,
        BTreeMap::from([(SECTOR_SIZE * 2, 0)])
    );
}

#[test]
fn rejects_missing_wel_before_program_or_erase() {
    for erase in [false, true] {
        let (mut flash, state) = initialized();
        state.borrow_mut().deny_wel = true;
        let result = if erase {
            run_ready(flash.erase(0, SECTOR_SIZE))
        } else {
            run_ready(flash.write(0, &[0x12]))
        };
        assert_eq!(result, Err(AsyncFlashError::WriteDisabled));
        assert_eq!(
            state.borrow().events,
            [Event::Status(0), Event::WriteEnable, Event::Status(0)]
        );
        assert!(state.borrow().memory.is_empty());
    }
}

#[test]
fn preserves_spi_errors_during_initialization() {
    for command in [0x05, 0x9F] {
        let (spi, delay, state) = peripherals();
        state.borrow_mut().fail_on = Some(command);
        assert!(matches!(
            run_ready(Flash::new(spi, delay)),
            Err(AsyncFlashError::Spi(SpiFault(actual))) if actual == command
        ));
    }
}

#[test]
fn preserves_spi_errors_from_all_command_paths() {
    for command in [0x05, 0x03, 0x06, 0x02, 0x20, 0x9F] {
        let (mut flash, state) = initialized();
        state.borrow_mut().fail_on = Some(command);
        let result = match command {
            0x05 | 0x03 => run_ready(flash.read(0, &mut [0])),
            0x06 | 0x02 => run_ready(flash.write(0, &[0])),
            0x20 => run_ready(flash.erase(0, SECTOR_SIZE)),
            0x9F => run_ready(flash.read_jedec_id()).map(|_| ()),
            _ => unreachable!(),
        };
        assert_eq!(result, Err(AsyncFlashError::Spi(SpiFault(command))));
    }
}

#[test]
fn initial_busy_timeout_is_bounded_and_does_not_read_id() {
    let (spi, delay, state) = peripherals();
    state.borrow_mut().stuck_busy = true;
    assert!(matches!(
        run_ready(Flash::new(spi, delay)),
        Err(AsyncFlashError::Timeout)
    ));
    let state = state.borrow();
    assert_eq!(state.events.len(), 10_000);
    assert!(state.events.iter().all(|event| *event == Event::Status(1)));
    assert_eq!(state.delays, vec![100_000; 10_000]);
}

#[test]
fn busy_timeout_prevents_access_and_bounds_post_program_and_erase_waits() {
    let (mut flash, state) = initialized();
    state.borrow_mut().stuck_busy = true;
    assert_eq!(
        run_ready(flash.read(0, &mut [0])),
        Err(AsyncFlashError::Timeout)
    );
    assert_eq!(state.borrow().events.len(), 10_000);
    assert_eq!(state.borrow().delays.len(), 10_000);

    for erase in [false, true] {
        let (mut flash, state) = initialized();
        state.borrow_mut().busy_after_command = 10_001;
        let result = if erase {
            run_ready(flash.erase(0, SECTOR_SIZE))
        } else {
            run_ready(flash.write(255, &[0; 2]))
        };
        assert_eq!(result, Err(AsyncFlashError::Timeout));
        let state = state.borrow();
        assert_eq!(state.delays, vec![100_000; 10_000]);
        assert_eq!(
            state
                .events
                .iter()
                .filter(|event| matches!(event, Event::WriteEnable))
                .count(),
            1
        );
        assert_eq!(state.events.len(), 10_004);
    }
}

#[test]
fn busy_poll_delay_is_awaited_and_can_yield_to_the_executor() {
    let (spi, delay, state) = peripherals();
    {
        let mut state = state.borrow_mut();
        state.busy_polls = 1;
        state.yield_delay = true;
    }
    let mut future = pin!(Flash::new(spi, delay));
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(state.borrow().events, [Event::Status(1)]);
    assert_eq!(state.borrow().delays, [100_000]);
    assert!(run_ready(future).is_ok());
    assert_eq!(
        state.borrow().events,
        [Event::Status(1), Event::Status(0), Event::Jedec]
    );
}

#[test]
fn spi_transactions_are_awaited_and_can_yield() {
    let (spi, delay, state) = peripherals();
    state.borrow_mut().yield_spi = true;
    let mut future = pin!(Flash::new(spi, delay));
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
    assert!(state.borrow().events.is_empty());
    assert!(future.as_mut().poll(&mut context).is_pending());
    assert_eq!(state.borrow().events, [Event::Status(0)]);
    assert!(run_ready(future).is_ok());
    assert_eq!(state.borrow().events, [Event::Status(0), Event::Jedec]);
}

#[test]
fn every_access_waits_after_a_canceled_program() {
    for next_operation in 0..4 {
        let (mut flash, state) = initialized();
        {
            let mut state = state.borrow_mut();
            state.busy_after_command = 2;
            state.yield_delay = true;
        }
        {
            let mut write = pin!(flash.write(0, &[0xAA]));
            let mut context = Context::from_waker(Waker::noop());
            assert!(write.as_mut().poll(&mut context).is_pending());
        }
        assert_eq!(state.borrow().busy_polls, 1);
        state.borrow_mut().events.clear();
        run_ready(async {
            match next_operation {
                0 => flash.read(0, &mut [0]).await,
                1 => flash.write(1, &[0x55]).await,
                2 => flash.erase(0, SECTOR_SIZE).await,
                3 => flash.read_jedec_id().await.map(|_| ()),
                _ => unreachable!(),
            }
        })
        .unwrap();
        let state = state.borrow();
        assert_eq!(&state.events[..2], &[Event::Status(1), Event::Status(0)]);
    }
}

#[test]
fn canceled_multi_sector_erase_does_not_start_the_next_sector() {
    let (mut flash, state) = initialized();
    {
        let mut state = state.borrow_mut();
        state.busy_after_command = 2;
        state.yield_delay = true;
    }
    {
        let mut erase = pin!(flash.erase(0, SECTOR_SIZE * 2));
        let mut context = Context::from_waker(Waker::noop());
        assert!(erase.as_mut().poll(&mut context).is_pending());
    }
    assert!(!state.borrow().events.contains(&Event::Erase(SECTOR_SIZE)));
    state.borrow_mut().events.clear();
    run_ready(flash.read(0, &mut [0])).unwrap();
    assert_eq!(
        state.borrow().events,
        [Event::Status(1), Event::Status(0), Event::Read(0, 1)]
    );
}

#[test]
fn satellite_storage_retains_typed_queues_and_native_postcard_settings_after_reopen() {
    use sat_drivers::storage::{Postcard, SatelliteStorage, StorageLayout};
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Telemetry {
        sequence: u32,
        battery_mv: u16,
        temperature_centi_c: i16,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct SubscriberPacket {
        subscriber_id: u16,
        sequence: u32,
        payload: [u8; 12],
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Config {
        telemetry_interval_s: u32,
        subscriber_id: u16,
        beacon_enabled: bool,
    }

    let telemetry = Telemetry {
        sequence: 42,
        battery_mv: 3710,
        temperature_centi_c: -125,
    };
    let packet = SubscriberPacket {
        subscriber_id: 17,
        sequence: 103,
        payload: *b"retained NOR",
    };
    let config = Postcard(Config {
        telemetry_interval_s: 60,
        subscriber_id: packet.subscriber_id,
        beacon_enabled: true,
    });
    let config_key = 7_u16;
    let layout = StorageLayout {
        telemetry: 0..2 * SECTOR_SIZE,
        client_packets: 2 * SECTOR_SIZE..4 * SECTOR_SIZE,
        settings: 4 * SECTOR_SIZE..6 * SECTOR_SIZE,
    };
    let (spi, delay, state) = peripherals();

    run_ready(async {
        let flash = AsyncW25Q64::new(spi, delay).await.unwrap();
        state.borrow_mut().events.clear();
        let mut stack = SatelliteStorage::new(flash, layout.clone()).unwrap();
        assert!(state.borrow().events.is_empty());

        let mut telemetry_scratch = [0_u8; 128];
        let mut packet_scratch = [0_u8; 128];
        let mut settings_scratch = [0_u8; 128];
        stack
            .save_telemetry(&telemetry, &mut telemetry_scratch)
            .await
            .unwrap();
        stack
            .save_client_packet(&packet, &mut packet_scratch)
            .await
            .unwrap();
        stack
            .settings()
            .store_item(&mut settings_scratch, &config_key, &config)
            .await
            .unwrap();

        telemetry_scratch.fill(0xA5);
        packet_scratch.fill(0xA5);
        settings_scratch.fill(0xA5);
        assert_eq!(
            stack
                .peek_telemetry::<Telemetry>(&mut telemetry_scratch)
                .await
                .unwrap(),
            Some(telemetry)
        );
        assert_eq!(
            stack
                .peek_client_packet::<SubscriberPacket>(&mut packet_scratch)
                .await
                .unwrap(),
            Some(packet)
        );
        assert_eq!(
            stack
                .settings()
                .fetch_item::<Postcard<Config>>(&mut settings_scratch, &config_key)
                .await
                .unwrap(),
            Some(config)
        );

        let retained_memory = state.borrow().memory.clone();
        let (spi, delay) = stack.into_inner().into_inner();
        assert!(Rc::ptr_eq(&spi.0, &state));
        assert!(Rc::ptr_eq(&delay.0, &state));
        state.borrow_mut().events.clear();
        let flash = AsyncW25Q64::new(spi, delay).await.unwrap();
        let mut reopened = SatelliteStorage::new(flash, layout).unwrap();
        assert_eq!(state.borrow().events, [Event::Status(0), Event::Jedec]);
        assert_eq!(state.borrow().memory, retained_memory);

        let mut telemetry_read_scratch = [0x5A_u8; 128];
        let mut packet_read_scratch = [0x5A_u8; 128];
        let mut settings_read_scratch = [0x5A_u8; 128];
        assert_eq!(
            reopened
                .peek_telemetry::<Telemetry>(&mut telemetry_read_scratch)
                .await
                .unwrap(),
            Some(telemetry)
        );
        assert_eq!(
            reopened
                .peek_client_packet::<SubscriberPacket>(&mut packet_read_scratch)
                .await
                .unwrap(),
            Some(packet)
        );
        assert_eq!(
            reopened
                .settings()
                .fetch_item::<Postcard<Config>>(&mut settings_read_scratch, &config_key)
                .await
                .unwrap(),
            Some(config)
        );
        let state = state.borrow();
        assert_eq!(state.memory, retained_memory);
        assert!(!state.events.iter().any(|event| matches!(
            event,
            Event::WriteEnable | Event::Program(..) | Event::Erase(_)
        )));
    });
}

#[test]
fn core_storage_adapter_retains_opaque_entries_and_isolates_client_clear_on_real_driver() {
    use sat_core::storage::{Storage, StorageErrorKind};
    use sat_drivers::storage::{Postcard, SatelliteStorage, SequentialQueue, StorageLayout};

    async fn append_and_check<S: Storage>(storage: &mut S, payload: &[u8]) {
        assert!(storage.max_entry_size().unwrap() >= payload.len());
        storage.append(payload).await.unwrap();
        let mut buffer = [0xA5; 512];
        for _ in 0..2 {
            let metadata = storage.peek(&mut buffer).await.unwrap().unwrap();
            assert_eq!(metadata.length(), payload.len());
            assert_eq!(&buffer[..metadata.length()], payload);
            buffer.fill(0x5A);
        }
        let mut small = [0xA5; 1];
        let error = storage.peek(&mut small).await.unwrap_err();
        assert_eq!(S::error_kind(&error), StorageErrorKind::BufferTooSmall);
        assert_eq!(small, [0xA5]);
        let metadata = storage.peek(&mut buffer).await.unwrap().unwrap();
        assert_eq!(metadata.length(), payload.len());
        assert_eq!(&buffer[..metadata.length()], payload);
    }

    let layout = StorageLayout {
        telemetry: SECTOR_SIZE..3 * SECTOR_SIZE,
        client_packets: 4 * SECTOR_SIZE..6 * SECTOR_SIZE,
        settings: 7 * SECTOR_SIZE..9 * SECTOR_SIZE,
    };
    let payload = b"\x00\xFF\x80opaque client packet\x00";
    let telemetry = b"\xFF\x00opaque telemetry\x80";
    let config_key = 7_u16;
    let config = Postcard(60_u32);
    let (spi, delay, state) = peripherals();

    run_ready(async {
        let mut flash = AsyncW25Q64::new(spi, delay).await.unwrap();
        for address in [
            0,
            layout.client_packets.start - 1,
            layout.client_packets.end,
            layout.settings.end,
            CAPACITY - 1,
        ] {
            flash.write(address, &[0xA5]).await.unwrap();
        }
        state.borrow_mut().events.clear();
        let mut queue =
            SequentialQueue::<_, 512>::new(flash, layout.client_packets.clone()).unwrap();
        assert!(state.borrow().events.is_empty());
        assert_eq!(queue.max_entry_size(), Some(512));
        append_and_check(&mut queue, payload).await;

        let retained_memory = state.borrow().memory.clone();
        let (spi, delay) = queue.into_inner().into_inner();
        assert!(Rc::ptr_eq(&spi.0, &state));
        assert!(Rc::ptr_eq(&delay.0, &state));
        state.borrow_mut().events.clear();
        let flash = AsyncW25Q64::new(spi, delay).await.unwrap();
        let mut stack = SatelliteStorage::new(flash, layout.clone()).unwrap();
        assert_eq!(state.borrow().events, [Event::Status(0), Event::Jedec]);
        assert_eq!(state.borrow().memory, retained_memory);

        let mut buffer = [0x5A; 512];
        {
            state.borrow_mut().events.clear();
            let mut queue = stack.client_packet_queue::<512>();
            assert!(state.borrow().events.is_empty());
            assert_eq!(queue.max_entry_size(), Some(512));
            let metadata = queue.peek(&mut buffer).await.unwrap().unwrap();
            assert_eq!(metadata.length(), payload.len());
            assert_eq!(&buffer[..metadata.length()], payload);
            assert!(queue.pop().await.unwrap());
        }

        let removed_memory = state.borrow().memory.clone();
        let (spi, delay) = stack.into_inner().into_inner();
        state.borrow_mut().events.clear();
        let flash = AsyncW25Q64::new(spi, delay).await.unwrap();
        let mut reopened = SatelliteStorage::new(flash, layout.clone()).unwrap();
        assert_eq!(state.borrow().events, [Event::Status(0), Event::Jedec]);
        assert_eq!(state.borrow().memory, removed_memory);
        {
            let mut queue = reopened.client_packet_queue::<512>();
            assert_eq!(queue.peek(&mut buffer).await.unwrap(), None);
            assert!(!queue.pop().await.unwrap());
            append_and_check(&mut queue, payload).await;
        }
        {
            state.borrow_mut().events.clear();
            let mut queue = reopened.telemetry_queue::<512>();
            assert!(state.borrow().events.is_empty());
            append_and_check(&mut queue, telemetry).await;
        }
        let mut settings_scratch = [0; 128];
        reopened
            .settings()
            .store_item(&mut settings_scratch, &config_key, &config)
            .await
            .unwrap();

        let protected_bytes = || {
            state
                .borrow()
                .memory
                .iter()
                .filter(|&(&address, _)| !layout.client_packets.contains(&address))
                .map(|(&address, &byte)| (address, byte))
                .collect::<BTreeMap<_, _>>()
        };
        let before_clear = protected_bytes();
        {
            let mut queue = reopened.client_packet_queue::<512>();
            queue.clear().await.unwrap();
            assert_eq!(queue.peek(&mut buffer).await.unwrap(), None);
            assert!(!queue.pop().await.unwrap());
        }
        assert_eq!(protected_bytes(), before_clear);
        assert!(
            state
                .borrow()
                .memory
                .range(layout.client_packets.clone())
                .next()
                .is_none()
        );
        let metadata = reopened
            .telemetry_queue::<512>()
            .peek(&mut buffer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(metadata.length(), telemetry.len());
        assert_eq!(&buffer[..metadata.length()], telemetry);
        assert_eq!(
            reopened
                .settings()
                .fetch_item::<Postcard<u32>>(&mut settings_scratch, &config_key)
                .await
                .unwrap(),
            Some(config)
        );
        assert_eq!(protected_bytes(), before_clear);
    });
}

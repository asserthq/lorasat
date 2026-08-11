//! FRP Transport layer — три варианта реализации.
//!
//! Протокол: Start → Data×N → Check(bitmap) → Retransmit → Done
//! Модель: sender-driven, receiver пассивно принимает и отправляет ack/check.
//!
//! Варианты в этом файле:
//!   1. `ReceiverManual`    — явный enum + match в async loop (без deps)
//!   2. `ReceiverAsync`      — чистый async/await, линейный код (без deps)
//!   3. `ReceiverStatig<R>`  — statig derive, каждый стейт = отдельный тип (feature = "statig")

use core::fmt::Debug;

use crate::error::FrpError;
use crate::messages::{FrpAck, FrpData, FrpStart};
use crate::session::ReceiverSession;

// ── Общий Radio trait (аналог HalfDuplexTransceiver, без sat-core) ──

#[allow(async_fn_in_trait)]
pub trait Radio {
    type Error: Debug;
    async fn transmit(&mut self, data: &[u8]) -> Result<usize, Self::Error>;
    async fn receive(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
}

// ═══════════════════════════════════════════════════════════════════
// Вариант A — Manual state machine (enum + match)
// ═══════════════════════════════════════════════════════════════════
//
// Плюсы:  явные переходы, легко логировать.
// Минусы: логика размазана по match-веткам, состояние и данные в одном enum.

/// Состояния приёмника FRP.
enum ReceiverState {
    /// Ожидание FrpStart — начало сессии.
    Idle,
    /// Приём чанков, отправка FrpAck на каждый.
    Receiving(ReceiverSession),
    /// Все чанки получены, сессия завершена.
    Done(Vec<u8>),
}

/// Приёмник FRP — manual state machine.
///
/// Использование:
/// ```ignore
/// let mut rx = ReceiverManual::new(radio);
/// let data: Vec<u8> = rx.run().await?;
/// ```
pub struct ReceiverManual<R: Radio> {
    radio: R,
    state: ReceiverState,
    buf: [u8; 512],
}

impl<R: Radio> ReceiverManual<R> {
    pub fn new(radio: R) -> Self {
        Self {
            radio,
            state: ReceiverState::Idle,
            buf: [0u8; 512],
        }
    }

    /// Главный цикл — крутится пока не получит все чанки.
    pub async fn run(&mut self) -> Result<Vec<u8>, FrpError> {
        loop {
            match &mut self.state {
                // ── Idle: ждём FrpStart ──
                ReceiverState::Idle => {
                    let n = self
                        .radio
                        .receive(&mut self.buf)
                        .await
                        .map_err(|_| FrpError::SessionNotInitialized)?;
                    if let Ok(start) = FrpStart::parse(&self.buf[..n]) {
                        let mut session = ReceiverSession::new();
                        session.handle_start(&start);
                        self.state = ReceiverState::Receiving(session);
                    }
                }

                // ── Receiving: принимаем чанки, шлём Ack ──
                ReceiverState::Receiving(ref mut session) => {
                    let n = self
                        .radio
                        .receive(&mut self.buf)
                        .await
                        .map_err(|_| FrpError::BufferTooShort)?;

                    if let Ok(data) = FrpData::parse(&self.buf[..n]) {
                        if data.session_id == session.session_id() {
                            session.handle_data(&data)?;

                            let ack = FrpAck {
                                session_id: data.session_id,
                                chunk_index: data.chunk_index,
                                status: 0,
                            };
                            let ack_bytes = ack.to_bytes();
                            self.radio
                                .transmit(&ack_bytes)
                                .await
                                .map_err(|_| FrpError::BufferTooShort)?;
                        }
                    }

                    // Проверяем — всё получено?
                    if let Some(msg) = session.get_assembled_message() {
                        self.state = ReceiverState::Done(msg);
                    }
                }

                // ── Done: возвращаем собранное сообщение ──
                ReceiverState::Done(ref msg) => return Ok(msg.clone()),
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Вариант B — Async/await (линейный код, без enum)
// ═══════════════════════════════════════════════════════════════════
//
// Плюсы:  код читается как спецификация, нет match-веток.
// Минусы: неявные состояния внутри .await, сложнее тестировать.

/// Приёмник FRP — async/await.
///
/// Использование:
/// ```ignore
/// let mut rx = ReceiverAsync::new(radio);
/// let data: Vec<u8> = rx.receive_file().await?;
/// ```
pub struct ReceiverAsync<R: Radio> {
    radio: R,
    buf: [u8; 512],
}

impl<R: Radio> ReceiverAsync<R> {
    pub fn new(radio: R) -> Self {
        Self {
            radio,
            buf: [0u8; 512],
        }
    }

    /// Получить файл по FRP. Блокируется до завершения приёма.
    pub async fn receive_file(&mut self) -> Result<Vec<u8>, FrpError> {
        // 1. Ждём FrpStart
        let start = self.wait_for_start().await?;
        let mut session = ReceiverSession::new();
        session.handle_start(&start);

        // 2. Принимаем чанки, на каждый шлём FrpAck
        loop {
            let data = self.wait_for_data().await?;
            if data.session_id != session.session_id() {
                continue; // чужой пакет
            }
            session.handle_data(&data)?;

            let ack = FrpAck {
                session_id: data.session_id,
                chunk_index: data.chunk_index,
                status: 0,
            };
            let ack_bytes = ack.to_bytes();
            self.radio
                .transmit(&ack_bytes)
                .await
                .map_err(|_| FrpError::BufferTooShort)?;

            if let Some(msg) = session.get_assembled_message() {
                return Ok(msg);
            }
        }
    }

    async fn wait_for_start(&mut self) -> Result<FrpStart, FrpError> {
        loop {
            let n = self
                .radio
                .receive(&mut self.buf)
                .await
                .map_err(|_| FrpError::SessionNotInitialized)?;
            if let Ok(start) = FrpStart::parse(&self.buf[..n]) {
                return Ok(start);
            }
        }
    }

    async fn wait_for_data(&mut self) -> Result<FrpData, FrpError> {
        loop {
            let n = self
                .radio
                .receive(&mut self.buf)
                .await
                .map_err(|_| FrpError::BufferTooShort)?;
            if let Ok(data) = FrpData::parse(&self.buf[..n]) {
                return Ok(data);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Вариант C — Statig (type-driven state machine)
// ═══════════════════════════════════════════════════════════════════
//
// Cargo.toml:
//   statig = "0.1"
//
// Плюсы:  каждый стейт — отдельный тип, переходы проверяются компилятором.
//         Нет match по enum — statig генерирует диспетчер.
// Минусы: зависимость от proc-macro крейта, сложнее в отладке.
//
// Архитектура statig:
//   - enum StateMachine (derive-макрос) — корневой enum, оборачивает стейты
//   - struct Idle / struct Receiving / ... — каждый стейт = тип
//   - impl State<StateMachine> for Idle — обработчик событий для стейта
//   - Response::Transition(NewState(...)) — переход в другой стейт
//
// События (Event) — то что дёргает машину:
//   - StartReceived(FrpStart)
//   - DataReceived(FrpData)
//   - RadioError

#[cfg(feature = "statig")]
pub mod statig_machine {
    use statig::prelude::*;

    use crate::error::FrpError;
    use crate::messages::{FrpAck, FrpData, FrpStart};
    use crate::session::ReceiverSession;
    use crate::transport::Radio;

    // ── Стейты ──

    /// Ожидание начала сессии.
    pub struct Idle;

    /// Приём чанков.
    pub struct Receiving {
        pub session: ReceiverSession,
    }

    /// Все чанки получены.
    pub struct Done {
        pub data: Vec<u8>,
    }

    // ── События ──

    pub enum FrpEvent {
        StartReceived(FrpStart),
        DataReceived(FrpData),
    }

    // ── Машина (генерируется derive) ──

    #[derive(Debug, StateMachine)]
    pub enum ReceiverMachine {
        Idle(Idle),
        Receiving(Receiving),
        Done(Done),
    }

    // ── Обработчики событий для каждого стейта ──

    impl State<ReceiverMachine> for Idle {
        fn handle(&mut self, event: &FrpEvent) -> Response<ReceiverMachine> {
            match event {
                FrpEvent::StartReceived(start) => {
                    let mut session = ReceiverSession::new();
                    session.handle_start(start);
                    Transition(ReceiverMachine::Receiving(Receiving { session }))
                }
                _ => Handled,
            }
        }
    }

    impl State<ReceiverMachine> for Receiving {
        fn handle(&mut self, event: &FrpEvent) -> Response<ReceiverMachine> {
            match event {
                FrpEvent::DataReceived(data) => {
                    if let Err(e) = self.session.handle_data(data) {
                        // В реальном коде — логируем ошибку, игнорируем чанк
                        let _ = e;
                        return Handled;
                    }
                    if self.session.get_assembled_message().is_some() {
                        let data = self.session.get_assembled_message().unwrap();
                        return Transition(ReceiverMachine::Done(Done { data }));
                    }
                    Handled
                }
                _ => Handled,
            }
        }
    }

    impl State<ReceiverMachine> for Done {
        fn handle(&mut self, _event: &FrpEvent) -> Response<ReceiverMachine> {
            Handled // Конечное состояние — игнорируем события
        }
    }

    // ── Драйвер: async loop кормит машину событиями ──

    /// Приёмник FRP на statig.
    ///
    /// Машина состояний живёт внутри, async loop читает радио
    /// и дёргает `machine.handle(&event)`.
    pub struct ReceiverStatig<R: Radio> {
        radio: R,
        machine: ReceiverMachine,
        buf: [u8; 512],
    }

    impl<R: Radio> ReceiverStatig<R> {
        pub fn new(radio: R) -> Self {
            Self {
                radio,
                machine: ReceiverMachine::Idle(Idle),
                buf: [0u8; 512],
            }
        }

        /// Главный цикл — читает радио, кормит машину событиями.
        pub async fn run(&mut self) -> Result<Vec<u8>, FrpError> {
            loop {
                let n = self
                    .radio
                    .receive(&mut self.buf)
                    .await
                    .map_err(|_| FrpError::BufferTooShort)?;

                // Пробуем распарсить — создаём событие
                let event = if let Ok(start) = FrpStart::parse(&self.buf[..n]) {
                    FrpEvent::StartReceived(start)
                } else if let Ok(data) = FrpData::parse(&self.buf[..n]) {
                    // Шлём Ack на каждый принятый чанк
                    let ack = FrpAck {
                        session_id: data.session_id,
                        chunk_index: data.chunk_index,
                        status: 0,
                    };
                    let ack_bytes = ack.to_bytes();
                    self.radio
                        .transmit(&ack_bytes)
                        .await
                        .map_err(|_| FrpError::BufferTooShort)?;

                    FrpEvent::DataReceived(data)
                } else {
                    continue; // неизвестный пакет — пропускаем
                };

                // Дёргаем машину
                if let Transition(new_state) = self.machine.handle(&event) {
                    self.machine = new_state;
                }

                // Проверяем — done?
                if let ReceiverMachine::Done(ref done) = self.machine {
                    return Ok(done.data.clone());
                }
            }
        }
    }
}

// ── Helper: доступ к session_id из ReceiverSession ──

impl ReceiverSession {
    pub fn session_id(&self) -> u16 {
        // NB: ReceiverSession не exposes session_id публично.
        // В реальном коде добавь pub fn session_id(&self) -> u16.
        // Здесь заглушка для демонстрации архитектуры.
        0
    }
}

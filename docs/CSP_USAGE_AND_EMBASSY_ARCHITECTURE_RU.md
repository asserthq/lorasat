# Подсистемы, communication traits, CSP и Embassy

Актуально для кода на 2026-10-29.

Документ описывает, как писать и запускать подсистемы, которым нужна связь. Главная граница для application code — traits из `sat_core::comm`. Подсистемы не должны зависеть от `sat-csp`, UDP, LoRa или конкретного wire format.

Детали CSP stack находятся в `csp/README.md`, фактическое состояние реализации — в `csp/STATUS.md`.

## 1. Основная архитектура

Подсистемы аппарата:

- **EPS** — питание, зарядка и energy modes;
- **ADCS** — sensors, orientation algorithms и coils;
- **Payload/Data Relay** — приём, хранение и выгрузка данных внешних терминалов;
- **Communication/Control** — beacon, telemetry, команды Земли и оркестрация подсистем.

Каждая подсистема представлена структурой или несколькими структурами, которые инкапсулируют:

- своё состояние;
- hardware, которым владеют;
- communication handles;
- command/event channels;
- один или несколько циклов `run()`.

`main` является composition root:

1. инициализирует MCU и peripherals;
2. создаёт static protocol resources и channels;
3. создаёт radios и CSP runners;
4. создаёт CSP sockets/listeners;
5. оборачивает CSP handles в adapters application traits;
6. передаёт каждой подсистеме её handles и channels;
7. создаёт подсистемы;
8. запускает infrastructure tasks и subsystem tasks.

Domain logic в `main` не размещается.

```text
radio
  |
CspInterface
  |
CSP Runner task
  |
sat_csp Socket / RdpListener / RdpConnection
  |
CSP adapter
  |
sat_core communication trait
  |
subsystem run()
```

## 2. Communication traits подсистем

Traits объявлены в `core/src/comm/`.

### `DatagramSocket`

Используется для коротких connectionless сообщений:

```rust,ignore
pub trait DatagramSocket {
    type Error: Debug;

    async fn send_to(
        &mut self,
        destination: Endpoint,
        payload: &[u8],
    ) -> Result<(), Self::Error>;

    async fn recv_from(
        &mut self,
        buffer: &mut [u8],
    ) -> Result<DatagramMetadata, Self::Error>;
}
```

Семантика:

- один вызов передаёт один datagram;
- границы сообщений сохраняются;
- соединение заранее не устанавливается;
- metadata содержит source address и source port;
- ACK/retry нет;
- потеря, duplication и reorder допустимы;
- успешный `send_to()` означает только принятие полного сообщения локальным transport;
- `recv_from()` cancellation-safe и может использоваться в Embassy `select`;
- слишком маленький receive buffer не должен давать успешное частичное сообщение.

Подходящие реализации:

- CSP connectionless socket через adapter;
- UDP через adapter;
- in-memory addressed queue для тестов;
- любой другой transport, который обеспечивает такую же семантику.

При CSP v2 и `MTU = 255` application payload занимает максимум 249 bytes.

### `ReliableMessageSession`

Представляет уже установленную надёжную message-oriented session с одним peer:

```rust,ignore
pub trait ReliableMessageSession {
    type Error: Debug;

    fn peer(&self) -> Endpoint;

    async fn send(&mut self, message: &[u8]) -> Result<(), Self::Error>;
    async fn recv(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error>;
}
```

Семантика:

- один `send()` передаёт одно сообщение;
- это не byte stream;
- границы сообщений сохраняются;
- session двунаправленная;
- transport отвечает за ACK/retry/ordering/duplicate suppression;
- `recv()` cancellation-safe;
- успешная transport delivery не означает исполнение application command.

Текущий CSP RDP даёт максимум 244 application bytes при `MTU = 255`. Он пока не является механизмом передачи большого блока.

### `SessionListener`

Получает входящие sessions на уже настроенном service endpoint:

```rust,ignore
pub trait SessionListener {
    type Error: Debug;
    type Session: ReliableMessageSession;

    async fn accept(&mut self) -> Result<Self::Session, Self::Error>;
}
```

Listener создаётся и bind-ится в `main`. Подсистема не выбирает local port.

### `SessionConnector`

Открывает исходящую session:

```rust,ignore
pub trait SessionConnector {
    type Error: Debug;
    type Session: ReliableMessageSession;

    async fn connect(
        &mut self,
        peer: Endpoint,
    ) -> Result<Self::Session, Self::Error>;
}
```

Connector нужен только подсистеме, которая сама инициирует session. Если Земля подключается к спутнику, спутнику обычно достаточно `SessionListener`: accepted session уже двунаправленная.

## 3. Communication handles хранятся прямо в подсистеме

Однополевые и малые вспомогательные `Links`/`Comms` структуры не нужны. Подсистема хранит каждый полученный socket/listener отдельным полем.

Например Communication/Control использует два независимых beacon sockets и listener команд Земли:

```rust,ignore
pub struct CommSystem<
    Beacon435Socket,
    Beacon868Socket,
    GroundCommandListener,
> {
    beacon_435_socket: Beacon435Socket,
    beacon_868_socket: Beacon868Socket,
    ground_command_listener: GroundCommandListener,
    adcs_commands: AdcsCommandSender,
    eps_commands: EpsCommandSender,
    payload_commands: PayloadCommandSender,
    system_events: SystemEventReceiver,
    state: CommState,
}
```

Названия generic parameters показывают роль handle. Короткое `Socket` допустимо, если структура содержит только один socket или роль ясна из контекста. Если sockets несколько, используются `Beacon435Socket`, `Beacon868Socket`, `TerminalSocket`, `HousekeepingSocket`.

```rust,ignore
impl<Beacon435Socket, Beacon868Socket, GroundCommandListener>
    CommSystem<Beacon435Socket, Beacon868Socket, GroundCommandListener>
where
    Beacon435Socket: DatagramSocket,
    Beacon868Socket: DatagramSocket,
    GroundCommandListener: SessionListener,
{
    pub fn new(
        beacon_435_socket: Beacon435Socket,
        beacon_868_socket: Beacon868Socket,
        ground_command_listener: GroundCommandListener,
        channels: CommChannels,
    ) -> Self {
        // сохранить handles, channels и state
    }
}
```

Разные fields нужны для независимых RX queues, ports, radios, mutable borrowing и mocks. Concrete Rust type у нескольких fields может быть одинаковым: разные generic names описывают их application roles.

Подсистема не получает общий `CspStack`, потому что тогда она смогла бы сама bind-ить произвольные ports и расходовать общий connection pool.

## 4. Пример PayloadSystem

Payload/Data Relay subsystem владеет:

- входным datagram endpoint диапазона 868 MHz;
- flash/storage;
- command receiver от Communication/Control;
- event sender;
- состоянием и counters.

```rust,ignore
pub struct PayloadSystem<TerminalSocket, TerminalStorage> {
    terminal_socket: TerminalSocket,
    terminal_storage: TerminalStorage,
    commands: PayloadCommandReceiver,
    events: SystemEventSender,
    state: PayloadState,
}

impl<TerminalSocket, TerminalStorage>
    PayloadSystem<TerminalSocket, TerminalStorage>
where
    TerminalSocket: DatagramSocket,
    TerminalStorage: Storage,
{
    pub async fn run(&mut self) -> ! {
        let mut frame_buffer = [0_u8; 249];
        let mut storage_buffer = [0_u8; TERMINAL_ENTRY_MAX_SIZE];

        loop {
            let received = self.terminal_socket.recv_from(&mut frame_buffer);
            let command = self.commands.receive();

            match select(received, command).await {
                Either::First(Ok(metadata)) => {
                    let entry_length = terminal_record_codec::encode(
                        metadata.source(),
                        mission_time(),
                        &frame_buffer[..metadata.length()],
                        &mut storage_buffer,
                    );

                    match entry_length {
                        Ok(length) => {
                            if let Err(error) = self
                                .terminal_storage
                                .append(&storage_buffer[..length])
                                .await
                            {
                                self.report_storage_error(error).await;
                            }
                        }
                        Err(error) => self.report_codec_error(error).await,
                    }
                }
                Either::First(Err(error)) => {
                    self.report_transport_error(error).await;
                }
                Either::Second(command) => {
                    self.handle_command(command).await;
                }
            }
        }
    }
}
```

Данные внешнего терминала остаются opaque. PayloadSystem сохраняет source, timestamp и bytes, но не обязан декодировать application payload терминала.

RDP на терминальной линии не используется. Повторную передачу после следующего beacon решает application logic терминала.

### Терминология подсистемы и внешних устройств

`PayloadSystem` — нормальное спутниковое название для mission-specific части аппарата, но оно слишком широкое для описания конкретного software service. Для нашего сценария возможны:

- `DataRelaySystem` — коротко, подчёркивает сбор и дальнейшую передачу данных;
- `StoreAndForwardSystem` — наиболее точно, но длиннее;
- `DataCollectionSystem` — хорошо описывает приём, слабее описывает выгрузку;
- `PayloadSystem` — оставить как верхнеуровневую subsystem, внутри иметь `DataRelayService`.

Внешнее абонентское устройство лучше называть `Terminal`: `TerminalSocket`, `TerminalData`, `TerminalStorage`. `Client` короче, но конфликтует с client/server ролью RDP и может обозначать Землю или спутник в разных sessions.

Центральный оркестратор обычно называют `CommandAndDataHandling` (`CdhSystem`), `MissionManager` или `FlightManager`. `PayloadSystem` не является названием главного оркестратора.

До отдельного решения документ сохраняет `PayloadSystem` как имя subsystem и использует `Terminal` для внешних устройств.

### Storage

`Storage` из `sat_core::storage` представляет одну bounded FIFO/ring область opaque entries:

```rust,ignore
storage.append(entry).await?;

if let Some(metadata) = storage.peek(&mut buffer).await? {
    process(&buffer[..metadata.length()]).await?;
    storage.pop().await?;
}
```

`peek()` не удаляет entry. Поэтому данные можно удалить только после успешной обработки или подтверждённой выгрузки. Запись не должна молча вытеснять старые entries: политика переполнения принадлежит владельцу конкретной области.

Platform-level данные разделяются на независимые области:

```rust,ignore
PlatformStorage<
    TelemetryStorage,
    EventStorage,
    DeferredCommandStorage,
    ConfigStorage,
>
```

- `telemetry`, `events`, `deferred_commands` обычно реализуют `Storage`;
- `configs` реализует `KeyValueStorage`, потому что config — key/value state, а не FIFO log;
- terminal data использует отдельный `TerminalStorage: Storage` прямо в PayloadSystem;
- однополевый `ServiceStorage<TerminalStorage>` не нужен.

Физически области могут быть partitions одной flash. Позже backend может использовать `sequential-storage`: queue для журналов/команд и map для configs.

## 5. Пример Communication/Control

Communication/Control отвечает за:

- beacon на 433/435 MHz;
- beacon на 868 MHz;
- приём команд Земли через reliable session;
- application codec команд и ответов;
- отправку typed commands другим подсистемам;
- сбор events/telemetry;
- ответы Земле.

Подсистему можно внутренне разделить на services:

```text
CommSystem
├── BeaconService
├── GroundCommandService
└── TelemetryService
```

Это остаётся одной логической подсистемой, но services могут выполняться независимыми futures или Embassy tasks.

### Отправка beacon

Codec изолирован от transport:

```rust,ignore
async fn send_beacon<Socket: DatagramSocket>(
    socket: &mut Socket,
    destination: Endpoint,
    beacon: &Beacon,
) -> Result<(), BeaconError<Socket::Error>> {
    let mut buffer = [0_u8; BEACON_MAX_SIZE];
    let length = beacon_codec::encode(beacon, &mut buffer)
        .map_err(BeaconError::Encode)?;

    socket
        .send_to(destination, &buffer[..length])
        .await
        .map_err(BeaconError::Transport)
}
```

Два диапазона используют разные handles:

```rust,ignore
send_beacon(
    &mut self.beacon_435_socket,
    Endpoint::new(BROADCAST_ADDRESS, BEACON_PORT),
    &beacon,
).await;

send_beacon(
    &mut self.beacon_868_socket,
    Endpoint::new(BROADCAST_ADDRESS, BEACON_PORT),
    &beacon,
).await;
```

Потеря отдельного периодического beacon допустима. Ошибка codec или постоянная transport error должна попасть в telemetry/event log.

### Приём команды Земли

```rust,ignore
async fn serve_one_ground_session<Listener>(
    listener: &mut Listener,
    channels: &mut CommChannels,
) -> Result<(), GroundServiceError>
where
    Listener: SessionListener,
{
    let mut session = listener
        .accept()
        .await
        .map_err(|_| GroundServiceError::Accept)?;

    let mut request_buffer = [0_u8; GROUND_MESSAGE_MAX_SIZE];
    let request_length = session
        .recv(&mut request_buffer)
        .await
        .map_err(|_| GroundServiceError::Session)?;

    let command = ground_codec::decode_command(
        &request_buffer[..request_length],
    )?;

    let response = dispatch_command(command, channels).await;

    let mut response_buffer = [0_u8; GROUND_MESSAGE_MAX_SIZE];
    let response_length = ground_codec::encode_response(
        &response,
        &mut response_buffer,
    )?;

    session
        .send(&response_buffer[..response_length])
        .await
        .map_err(|_| GroundServiceError::Session)
}
```

Одна accepted session двунаправленная:

```text
Ground connect
Ground send command
Satellite recv command
Satellite dispatch typed command
Subsystem returns event/result
Satellite send response через ту же session
```

Satellite не нужен `SessionConnector` для ответа в этой session.

Для долгой операции команда должна содержать `command_id`. Короткий ответ может быть `Accepted { command_id }`; результат позже передаётся отдельным сообщением после расширения RDP lifecycle или через будущий механизм telemetry/downlink.

## 6. Межподсистемный обмен

CSP не используется для связи задач внутри одного MCU. Подсистемы общаются typed Embassy channels.

```rust,ignore
pub enum SystemEvent {
    Adcs(AdcsEvent),
    Eps(EpsEvent),
    Payload(PayloadEvent),
}
```

```text
CommSystem --AdcsCommand----> ADCS
CommSystem --EpsCommand-----> EPS
CommSystem --PayloadCommand-> PayloadSystem
CommSystem <--SystemEvent---- subsystems
```

Рекомендуемые primitives:

- `Channel<Command>` — bounded очередь одному consumer;
- `Channel<Event>` — события оркестратору;
- `PubSubChannel` — события нескольким consumers;
- `Watch<StateSnapshot>` — только последнее состояние;
- `Mutex` — только для действительно shared state.

Flash лучше отдать PayloadSystem единолично. CommSystem запрашивает records typed command, а не получает flash mutex.

## 7. Создание CSP handles и adapters в main

CSP остаётся infrastructure implementation communication traits.

Static resources:

```rust,ignore
static STACK_435: StackResources<255, 8, 2, 4> = StackResources::new();
static STACK_868: StackResources<255, 4, 2, 4> = StackResources::new();
static RDP_435: RdpResources<255> = RdpResources::new();
```

Создание 433/435 stack:

```rust,ignore
let mut builder_435 = RunnerBuilder::new(
    SATELLITE_ADDRESS,
    radio_435,
    &STACK_435,
)?;

let beacon_435_csp = builder_435.bind(BEACON_PORT)?;
let ground_listener_csp = builder_435.listen_rdp(
    COMMAND_PORT,
    &RDP_435,
    RdpConfig::new(initial_sequence),
)?;

let runner_435 = builder_435.build();
```

Создание 868 stack:

```rust,ignore
let mut builder_868 = RunnerBuilder::new(
    SATELLITE_ADDRESS,
    radio_868,
    &STACK_868,
)?;

let beacon_868_csp = builder_868.bind(BEACON_PORT)?;
let terminal_csp = builder_868.bind(CLIENT_DATA_PORT)?;
let runner_868 = builder_868.build();
```

CSP handles оборачиваются до передачи подсистемам:

```rust,ignore
let beacon_435_socket = CspDatagramAdapter::new(beacon_435_csp);
let beacon_868_socket = CspDatagramAdapter::new(beacon_868_csp);
let terminal_socket = CspDatagramAdapter::new(terminal_csp);
let ground_command_listener =
    CspSessionListenerAdapter::new(ground_listener_csp);
```

Adapters пока являются следующим этапом реализации. Они должны:

- преобразовать `Endpoint` в CSP address/port и обратно;
- проверить допустимый диапазон CSP address/port;
- скопировать принятое сообщение в caller-provided buffer;
- не выдавать truncated message как успешное;
- преобразовать CSP errors в adapter errors;
- сохранить cancellation safety.

Adapter не содержит:

- application codec;
- beacon scheduling;
- command dispatch;
- subsystem state;
- radio/MAC logic.

`sat-csp` не зависит от `sat-core`. Поэтому adapters должны быть локальными types в integration crate/module, который зависит от обоих crates.

## 8. Сборка и запуск подсистем в main

Псевдокод composition root:

```rust,ignore
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let peripherals = init_mcu();

    let radio_435 = create_radio_435(/* SPI1, DMA, pins, EXTI */).await;
    let radio_868 = create_radio_868(/* SPI2, DMA, pins, EXTI */).await;

    let BuiltCspLinks {
        runner_435,
        runner_868,
        beacon_435_socket,
        beacon_868_socket,
        terminal_socket,
        ground_command_listener,
    } = build_csp_links(radio_435, radio_868).await;

    let terminal_storage = create_terminal_storage();

    let payload = PayloadSystem::new(
        terminal_socket,
        terminal_storage,
        PAYLOAD_CHANNELS.command_receiver(),
        SYSTEM_EVENTS.sender(),
    );

    let comm = CommSystem::new(
        beacon_435_socket,
        beacon_868_socket,
        ground_command_listener,
        CommChannels {
            adcs_commands: ADCS_CHANNELS.command_sender(),
            eps_commands: EPS_CHANNELS.command_sender(),
            payload_commands: PAYLOAD_CHANNELS.command_sender(),
            system_events: SYSTEM_EVENTS.receiver(),
        },
    );

    let adcs = AdcsSystem::new(/* hardware + channels */);
    let eps = EpsSystem::new(/* hardware + channels */);

    // Infrastructure tasks владеют radios и protocol state.
    spawner.spawn(run_csp_435(runner_435)).unwrap();
    spawner.spawn(run_csp_868(runner_868)).unwrap();
    spawner.spawn(run_rdp_clock(&RDP_435)).unwrap();

    // Domain tasks владеют subsystem logic и trait handles.
    spawner.spawn(run_payload(payload)).unwrap();
    spawner.spawn(run_comm(comm)).unwrap();
    spawner.spawn(run_adcs(adcs)).unwrap();
    spawner.spawn(run_eps(eps)).unwrap();
}
```

Результат `build_csp_links` сразу разбирается на runners и handles. После этого каждый ресурс перемещается ровно одному владельцу.

Embassy `#[task]` wrappers обычно должны иметь concrete types. Generic subsystem остаётся в `subsystems`, а firmware задаёт concrete type alias или небольшой concrete task wrapper.

```rust,ignore
type FlightPayloadSystem = PayloadSystem<FlightTerminalSocket, FlightTerminalStorage>;

#[embassy_executor::task]
async fn run_payload(mut system: FlightPayloadSystem) {
    system.run().await
}
```

`StaticCell` используется, когда concrete runner/handle или task storage требует `'static`. `static mut` не нужен.

## 9. Реализация новых передач

Новая передача добавляется в четыре шага.

### 1. Определить семантику

- короткое best-effort сообщение — `DatagramSocket`;
- короткий надёжный request/response — `SessionListener`/`SessionConnector` и `ReliableMessageSession`;
- большой блок — будущий отдельный fragmentation/block-transfer trait, не обычный datagram;
- обмен внутри MCU — typed Embassy channel.

### 2. Назначить service endpoint

Ports хранятся централизованно:

```text
10  BEACON_PORT
11  CLIENT_DATA_PORT
20  COMMAND_PORT
63  внутренний текущий RDP client port
```

Новые ports не должны быть разбросаны literals по подсистемам.

### 3. Написать codec отдельно

```text
comm/codec/beacon.rs
comm/codec/ground_command.rs
comm/codec/ground_response.rs
comm/codec/housekeeping.rs
```

Codec получает typed value и byte buffer. Можно использовать ручной layout или `postcard`.

```rust,ignore
let length = codec::encode(&value, &mut buffer)?;
socket.send_to(destination, &buffer[..length]).await?;
```

```rust,ignore
let metadata = socket.recv_from(&mut buffer).await?;
let value = codec::decode(&buffer[..metadata.length()])?;
```

Serialization не должна проникать в Runner, CSP headers, RDP state или radio driver.

### 4. Добавить handle полем нужной подсистемы

```rust,ignore
pub struct CommSystem<
    Beacon435Socket,
    Beacon868Socket,
    GroundCommandListener,
    HousekeepingSocket,
> {
    beacon_435_socket: Beacon435Socket,
    beacon_868_socket: Beacon868Socket,
    ground_command_listener: GroundCommandListener,
    housekeeping_socket: HousekeepingSocket,
    // channels and state
}
```

`main` создаёт endpoint и adapter, затем передаёт handle прямо в constructor подсистемы.

## 10. Тестирование подсистем через traits

Subsystem unit/integration tests не должны запускать CSP Runner или LoRa.

Mock реализует тот же trait:

```rust,ignore
struct MockDatagramSocket {
    incoming: Queue<TestDatagram>,
    sent: Queue<TestDatagram>,
}

impl DatagramSocket for MockDatagramSocket {
    // send_to сохраняет сообщение в sent
    // recv_from забирает одно сообщение из incoming
}
```

Тест PayloadSystem:

```rust,ignore
#[test]
fn terminal_data_is_stored_opaque() {
    let terminal_socket = MockDatagramSocket::with_incoming(
        Endpoint::new(Address(42), CLIENT_DATA_PORT),
        b"terminal bytes",
    );
    let terminal_storage = MockStorage::new();
    let mut payload = PayloadSystem::new(
        terminal_socket,
        terminal_storage,
        /* channels */
    );

    run_ready(payload.process_one_event());

    assert_eq!(
        payload.terminal_storage.entries()[0],
        b"terminal bytes"
    );
}
```

Тест Communication/Control:

- mock listener возвращает mock reliable session;
- session содержит encoded command;
- CommSystem отправляет typed command в mock/channel;
- тест подаёт subsystem result;
- проверяет encoded response в mock session.

Отдельные уровни тестируются отдельно:

```text
Subsystem tests      traits + mocks, без CSP
Adapter tests        sat_core traits поверх sat_csp handles
CSP tests            headers, routing, queues, RDP state
Driver tests         radio driver без CSP
Hardware examples    реальный SPI/LoRa, ручной запуск
End-to-end examples  UDP mock + CSP + subsystem scenario
```

## 11. Большая выгрузка Payload

Текущие traits и текущий RDP не решают fragmentation большого блока.

Сейчас возможно:

```text
короткая команда Земли
короткий response
короткий record/message до RDP limit
```

Целевая большая выгрузка:

```text
Ground отправляет StartDump
CommSystem отправляет PayloadCommand
PayloadSystem читает flash порциями
block-transfer/SFP service фрагментирует данные
reliable transport подтверждает необходимые части
MAC оставляет command RX windows
```

После проектирования SFP/block transfer появится отдельный application-facing handle. Он будет добавлен отдельным полем владельцу выгрузки. Обычный `DatagramSocket` не должен изображать передачу большого блока.

## 12. Runner, radio и будущий MAC

Один Runner владеет одним `CspInterface`, сейчас непосредственно реализованным radio adapter.

```text
subsystem handles
       |
shared CSP TX/RX resources
       |
Runner
       |
CspInterface
       |
LoRa radio
```

Radio не раздаётся подсистемам и не делится между ними через mutex.

Будущий MAC вставляется под Runner без изменения subsystem traits:

```text
Runner
  |
CspInterface implemented by MAC
  |
MAC scheduling / RX windows / duty cycle
  |
LoRa radio
```

Interface error завершает `Runner::run()`. Recovery выполняет infrastructure task, владеющая Runner:

```rust,ignore
loop {
    if let Err(error) = runner.run().await {
        report_comm_fault(error);
        recover_interface(&mut runner).await;
    }
}
```

Подсистемы не reset-ят radio.

## 13. Текущее состояние и ограничения

Уже есть:

- CSP v2 header;
- connectionless sockets;
- несколько bound ports;
- bounded queues;
- Runner/RunnerBuilder;
- минимальный RDP handshake, ACK, retry и timeout;
- application-facing communication traits в `sat-core`;
- host CSP examples и raw LoRa hardware examples.

Ещё нет:

- CSP adapters для новых `sat-core` traits;
- reusable RDP connections;
- нескольких одновременных RDP connections;
- нескольких сообщений в одной текущей connection;
- SFP/block fragmentation;
- MAC scheduling/RX windows;
- production LoRa recovery.

Текущий RDP поддерживает одну connection и один `send()` на handle. Архитектура подсистем не должна рассчитывать на долгоживущую многосообщенческую session до расширения реализации.

## 14. Главное правило

`main` создаёт hardware, CSP resources, adapters, trait handles и channels. Runner tasks владеют radios и protocol state. Подсистемы получают только необходимые communication traits и typed channels, реализуют application transfers и не знают, работает под ними CSP, UDP mock или будущий другой transport.

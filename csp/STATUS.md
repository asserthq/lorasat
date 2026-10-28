# Текущее состояние

Этот файл обновляет агент. Пользователю ничего заранее заполнять не нужно.

## Где остановились

- Этап 1 — формат пакета CSP v2: завершён.
- Этап 2 — Runner и ненадёжные датаграммы: завершён.
- Работают два узла с async Runner, bounded Socket, request/reply, потери и дубликаты.
- Этап 3, первый RDP profile: codec, handshake, один DATA/ACK, retry и bounded timeout завершены.
- Реализован один RDP endpoint и один `RdpConnection` на узел; `send` завершается только после ACK peer.
- Следующий отдельный подшаг: close/reset и безопасная отмена. Несколько сообщений в connection и несколько connections явно отложены.

## Настройки проекта

- Библиотека: `csp/`, package `sat-csp`, Rust edition 2024.
- `csp/` добавлен в members и default-members корневого workspace.
- Runtime-зависимости: `embassy-sync 0.8.0` и `embassy-futures 0.1.2`; executor, allocator, workspace crates и `unsafe` не используются.
- Эталон: libcsp v2.1, commit `48f7fb0b57f610bf65bab1aa2d1357c3b9722782`.
- Wire-format: `src/csp_id.c`; поля и флаги: `include/csp/csp_types.h` этого commit.

## Принятые решения

- Формат CSP v2, без поддержки v1 на первых этапах.
- Ядро no_std, тесты на компьютере под std.
- Простой код важнее оптимизаций.
- Runner владеет Link, циклом RX/TX, port routing и счётчиками; приложение запускает `Runner::run` само.
- Библиотека в `csp/` не зависит от других crates workspace: drivers, ядра логики аппарата и firmware.
- `Link` отправляет/принимает целый frame и не обязан делать RX/TX одновременно. `receive` должен быть cancellation-safe для переключения полудуплексного радио.
- Представление packet: простой owned `Packet<const MTU: usize>` без lifetime; копирование небольшого payload допустимо.
- Wire serialization сосредоточена в `src/wire.rs`; остальной код работает с `Header` и `Packet`.
- Codec вручную повторяет masks/offsets/network byte order libcsp CSP v2; postcard не используется.
- Header — 6 bytes. `Packet<const MTU: usize>` владеет payload; рекомендуемый `DEFAULT_MTU` — 255 bytes целого packet.
- `Packet` принимает `Flags::NONE` и `Flags::RDP`; CRC32, HMAC и fragmentation возвращают `UnsupportedFlags` до реализации соответствующего слоя.
- Fixture generator `tools/libcsp_v2_vectors.c` вызывает реальный `csp_id_prepend`; лицензия upstream сохранена в `THIRD_PARTY_LICENSES.md`.
- C-generator остаётся минимальным воспроизводимым инструментом этапа 1. Если понадобится больше C/C++ interoperability tooling, оно выносится в отдельный проект с зависимостью от libcsp, не расширяется внутри firmware workspace.
- `StackResources<MTU, TX_DEPTH, SOCKETS, RX_DEPTH>` задаёт всю память очередей; `RunnerBuilder` привязывает уникальные порты до запуска и затем уничтожается.
- Каналы используют `CriticalSectionRawMutex`. `send_to` завершается после локальной постановки в TX; `try_send_to` возвращает `Busy`; полный RX Socket отбрасывает новый packet без блокировки Runner.
- Datagram не создаёт ACK/retry. Ошибочный frame, чужой destination и закрытый port отбрасываются со счётчиками. Runner чередует приоритет RX/TX при одновременной готовности.
- Ожидаемые события канала обрабатывает Link внутри. `Link::Error` означает невозможность продолжать: Runner возвращает ошибку supervisor-задаче без скрытого retry. После остановки доступны повторный `run()` и `link_mut()` для link-specific recovery/reset.
- Собираемые datagram-сценарии зафиксированы в `examples/std/src/bin/csp_datagram.rs` и `examples/stm32f401/src/bin/csp_datagram.rs`; RDP-сценарии — в соответствующих `csp_rdp.rs`. Host и RDP MCU example используют два memory-link узла; реального радио нет.
- У аппарата два независимых полудуплексных трансивера: 868 MHz для маяка и абонентских данных, 435 MHz для маяка/housekeeping, команд и будущего длительного downlink.
- MAC Runner, гарантированные RF RX-окна и одновременная организация RX/TX сейчас не проектируются. Текущий Runner даёт только software fairness; при непрерывном TX он не гарантирует окно приёма команды.
- Сценарий 868 MHz: спутник раз в 10 секунд отправляет короткий broadcast beacon; абонент извлекает адрес, время и интервал, выбирает случайный слот и отправляет один unicast datagram.
- Сценарий 435 MHz пока использует beacon datagram и один короткий RDP command/response. Большой downlink требует будущей SFP-фрагментации; фиксированные command RX windows потребуют отдельного MAC/scheduler слоя.
- Трёхпроцессный host-сценарий находится в `examples/std/src/bin/{ground,satellite,client}.rs`; общий UDP Link и простые codecs — в `examples/std/src/csp_demo.rs`.
- UDP mock разделяет эфиры портами: 435 MHz — `14350 + CSP address`, 868 MHz — `18680 + CSP address`. Satellite CSP=1, ground=2, client=3.
- Beacon — manual fixed layout: version, satellite address, Unix time, interval. Subscriber отправляет opaque datagram; satellite хранит source/source port/raw bytes без разбора.
- RDP `dump` возвращает простой envelope записей в одном DATA packet. Если все записи не помещаются в 244 application bytes, ответ содержит только поместившиеся и исходный total count; SFP пока не применяется.
- SFP отложен до завершения RDP. В libcsp SFP metadata добавляется trailer после chunk: payload остаётся с byte 0, добавление/снятие не требует `memmove`. Тем же способом libcsp добавляет RDP, HMAC и CRC32 trailers; внешний слой снимается первым.
- RDP повторяет wire/state semantics закреплённого libcsp. Не добавлять postcard, собственные OPEN/READY/DONE или отсутствующий в libcsp session ID.
- `RdpResources` отделены от datagram `StackResources`; Runner выбирает Link RX, datagram TX, RDP command и time update с ротацией приоритета.
- Первый профиль: window 1, delayed ACK off, client source port 63, один `send` на handle. Initial sequence задаёт приложение.
- `RdpResources::update_time(u32)` подаёт wrapping monotonic milliseconds. Packet retry — 1000 ms, bounded operation timeout — 10000 ms. Clock нужно инициализировать до `connect` и обновлять периодически.
- `RdpConnection::send` ждёт cumulative ACK. ACK означает помещение DATA в peer RDP RX queue, не исполнение команды.
- Потерянные SYN/DATA повторяются. Потерянный DATA ACK вызывает повтор DATA без повторной app delivery. DATA может завершить server handshake при потере финального чистого ACK, как в общем `SYN_RCVD` path libcsp.
- Пока нет close/RST API, EAK/out-of-order buffering, reuse connection, cleanup отменённого future, нескольких сообщений или нескольких connection slots.

## Изучено для этапа 3

По `src/csp_rdp.c` закреплённого commit:

- RDP trailer — 5 bytes: `flags: u8`, `seq_nr: u16 BE`, `ack_nr: u16 BE`.
- Значимые control bits находятся в младшей половине flags: `SYN=0x08`, `ACK=0x04`, `EAK=0x02`, `RST=0x01`. libcsp меняет старшие 4 bits control packet, чтобы CSP deduplication не удалял одинаковые подтверждения.
- SYN payload перед trailer — шесть `u32 BE`: window size, connection timeout, packet timeout, delayed ACK, ACK timeout, ACK delay count.
- Состояния upstream: `CLOSED`, `SYN_SENT`, `SYN_RCVD`, `OPEN`, `CLOSE_WAIT`.
- Handshake: `SYN(ISS)` → `SYN|ACK(server ISS, client ISS)` → `ACK(client ISS+1, server ISS)`. Pure ACK не расходует sequence number; DATA использует `snd_nxt` и затем увеличивает его.
- Каждый DATA содержит ACK и тем самым подтверждает принятые данные в обратном направлении. ACK cumulative: `snd_una = ack_nr + 1`.
- `window_size = 1` допустим: отдельной проверки/минимума нет, а TX блокируется после одного неподтверждённого DATA. `delayed_acks = 0` явно поддержан и заставляет `csp_rdp_should_ack` немедленно вернуть true.
- Upstream `csp_send` не ждёт ACK текущего DATA: он ждёт свободное окно, сохраняет копию для retry, передаёт packet вниз и возвращается. Наш Rust API намеренно строже: первый `send` ждёт ACK.
- Retransmit запускается периодическим `csp_rdp_check_timeouts`; sequence/time сравниваются через wrapping subtraction, не обычными `<`/`>`.
- Первый профиль использует window 1 и delayed ACK off. EAK/out-of-order buffering не реализованы; peer/address/port и wrapping sequence проверяются до принятия ACK.

## Последняя проверка

Все проверки после последнего изменения прошли:

- `cargo test -p sat-csp` — 27 unit/integration tests и doc-tests успешно.
- `cargo fmt -p sat-csp -- --check` — успешно.
- `cargo clippy -p sat-csp --all-targets -- -D warnings` — успешно.
- `cargo check -p sat-csp --target thumbv7em-none-eabihf` — успешно.
- `cargo doc -p sat-csp --no-deps` — успешно.
- `cargo check -p examples-std --bin csp_datagram` — успешно; `cargo run` передал `hello`/`world` и завершился.
- `cargo run -p examples-std --bin csp_rdp` — успешно; handshake, confirmed command и receive завершились.
- `cargo check -p examples-std --bin ground --bin satellite --bin client` — успешно.
- Реальный запуск трёх host-процессов прошёл: оба beacon получены, client отправил 22 opaque bytes, satellite сохранил запись, ground через RDP `dump` получил ту же запись byte-for-byte.
- `cargo test -p examples-std --lib csp_demo` — 2 codec tests успешно.
- `cargo check -p examples-stm32f401 --bin csp_datagram --target thumbv7em-none-eabihf` — успешно.
- `cargo check -p examples-stm32f401 --bin csp_rdp --target thumbv7em-none-eabihf` — успешно.
- CSP example-файлы проходят `rustfmt --check`. Package-wide format для `examples/stm32f401` заблокирован существующей синтаксической ошибкой в `src/bin/tx_flash/main.rs:127`, не связанной с CSP.
- C fixture generator собран с `-Wall -Wextra -Werror`; bytes совпадают с Rust tests.
- `datagram_request_reply` проверяет `hello`/`world`, wire bytes, адреса, порты и по одной физической передаче. Отдельный Rust ↔ libcsp datagram harness не запускался: по принятому решению такой стенд должен жить в отдельном C/C++ проекте.

## Открытые вопросы

Перед следующим RDP изменением согласовать close/reset и cleanup отменённых `connect`/`send`; сейчас отмена может оставить занятый первый slot. Initial sequence пока задаёт приложение, автоматический entropy source отсутствует. Не проверены на железе cancellation safety текущего `lora_phy`, реальное переключение RX/TX и способность Link восстанавливаться после отменённого `receive`; проверить при интеграции с радио.

---

Правило обновления: сохранять краткую актуальную выжимку, а не журнал всех сообщений. После каждого законченного подшага и перед переходом в новый чат обновлять прогресс, решения, проверки и следующий шаг. История остаётся в git и чате. Ориентир — до одной страницы, при необходимости немного больше.

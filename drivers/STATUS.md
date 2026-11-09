# Состояние drivers

Актуально на 2026-11-04.

## Новый SX1262 CSP interface

Использовать `sat_drivers::lora::Sx1262CspInterface` из `src/lora/csp_sx1262.rs`.

```rust,ignore
let control = Sx1262Control {
    reset,
    dio1,
    busy,
    rf_switch_rx: None,
    rf_switch_tx: None,
};

let mut config = Sx1262CspConfig::new(FREQUENCY_HZ, MTU as u8);
config.spreading_factor = SpreadingFactor::_9;
config.bandwidth = Bandwidth::_125KHz;
config.coding_rate = CodingRate::_4_5;
config.tx_power_dbm = 2;

let interface = Sx1262CspInterface::new(
    spi_device,
    control,
    Delay,
    config,
)
.await?;

let runner = RunnerBuilder::new(
    SATELLITE_ADDRESS,
    interface,
    &STACK_RESOURCES,
)?
.build();
```

Один LoRa packet содержит один полный CSP frame. Adapter не выполняет CSP parsing, routing, retries, fragmentation или MAC scheduling.

`max_frame_size` должен совпадать с `MTU` Runner и не превышать 255 bytes. Receive buffer меньше этого размера отклоняется до обращения к radio.

Настройки `TCXO`, `use_dcdc`, RF switch, frequency и TX power являются board-specific. Значения `Sx1262CspConfig::new()` подходят только как bring-up defaults и должны быть сверены со схемой модуля.

SX1268 управляется через `lora_phy::sx126x::Sx1262`, но его RF диапазон не включает 868 MHz. Для 868 MHz нужен соответствующий SX1262-class модуль.

## Стек хранения на готовых библиотеках

Новый путь: `serde/postcard` + `sequential-storage 8.0.2` +
`embedded-storage-async 0.4` + `AsyncW25Q64` + W25Q64.

- `src/flash/asynchronous.rs` реализует стандартные `ReadNorFlash`, `NorFlash`,
  `MultiwriteNorFlash` через async SPI и async ожидание готовности.
- `src/storage/sequential.rs` содержит `SatelliteStorage`: независимые области
  телеметрии, данных АСП и Map конфигурации/состояния, проверка разметки,
  типизированные Queue-операции и штатная интеграция `Postcard<T>` с Map.
- Готовая библиотека отвечает за размещение, CRC, циклическое использование памяти
  и восстановление; собственный Flash-журнал для этого пути не пишется.
- `CommSystem`, протокол НС и бортовая разводка Flash не изменены.

Пример и ограничения: [STORAGE_STACK_RU.md](../docs/STORAGE_STACK_RU.md).

## Ранее реализованные Storage API


### RAM

`src/storage/ram.rs`:

- `RamStorage<ENTRY_SIZE, DEPTH>` реализует `sat_core::storage::Storage`;
- `RamKeyValueStorage<VALUE_SIZE, KEYS>` реализует `KeyValueStorage`;
- используется в subsystem tests и до подключения flash.

### Удалённые прототипы

Неиспользуемые экспериментальный RAM-журнал и самодельное секторное Flash-хранилище
удалены вместе с экспортами, отдельными тестами и устаревшей документацией.
`Storage`, `RamStorage`, `RamKeyValueStorage` и их тесты сохранены для существующих
потребителей. Реализации старого трейта `KeyValueStorage` для W25Q64 нет;
новый стек предоставляет key/value через готовый `MapStorage` и `Postcard<T>`.

## Критичные устаревшие места

### Legacy protocol stack не собирается

`drivers/src/proto_impl/` зависит от удалённых `sat_core::comm::LinkLayer`, `RecvFrame`, `TransportLayer`, `TransportEvent` и `Socket`.

Legacy module сохранён под feature:

```toml
legacy-protocol = []
```

Команда ожидаемо падает:

```text
cargo check -p sat-drivers --features legacy-protocol
unresolved import sat_core::comm::link
```

Это не исправляется парой import changes: модель `LinkLayer -> TransportLayer` заменена CSP Runner и application-facing communication traits. Код нужно мигрировать или удалить после переноса оставшихся пользователей.

Затронуты:

- `client-device/src/main.rs` и `client-device/src/device.rs`;
- `ground-station/src/main.rs` и legacy transport mocks;
- `examples/std/tests/transport_srv_chunking.rs`;
- `examples/stm32f401/src/bin/tx_flash/main.rs`.

`client-device` сейчас не собирается и требует перехода на простой CSP beacon decoder/datagram sender. `ground-station` требует перехода на `sat-csp` или новые core traits.

### Legacy FlashRing нельзя использовать как новый Storage

`drivers/src/flash/ring.rs` оставлен без изменений, но его address model непоследователен:

- до `recover()` `head`/`tail` используются как offsets;
- `recover()` записывает в них absolute addresses;
- `append()`, `head()` и `tail()` повторно прибавляют `base`;
- `rewind()`/`read_next()` могут читать address `0` вместо `base`;
- нет `pop()` и `clear()`;
- overwrite не двигает head и может уничтожить unread data.

Это критично для ненулевого `base` и после recovery. Старый ring оставлен для аппаратных примеров; новый код должен использовать `SatelliteStorage` поверх готовых Queue/Map `sequential-storage`.

### Flash operations блокируют Embassy executor

Старый `W25Q64` использует blocking `embedded_hal::spi::SpiDevice` и polling `DelayNs`, поэтому SPI/program/erase в прежних аппаратных примерах блокируют executor.

Новый стек использует `AsyncW25Q64` с асинхронными SPI и ожиданием готовности. Старые примеры ещё не перенесены; для них прежнее ограничение сохраняется.

### lora-phy 3.0.1 может panic при плохом SPI response

В `lora-phy-3.0.1/src/sx126x/mod.rs` retention-list count используется до проверки диапазона. При неверном SPI response наблюдался panic `index out of bounds: the len is 9 but the index is 9`.

Новый adapter не может исправить panic внутри dependency. Перед hardware integration нужен upgrade/patch `lora-phy` и проверка BUSY/MISO/NSS/RESET/TCXO wiring.

### Cancellation safety radio не проверена на hardware

`CspInterface::receive()` может быть отменён Runner перед TX. Adapter заново вызывает `prepare_for_tx`/`prepare_for_rx`, но способность `lora-phy` и конкретного SX1262 восстанавливаться после отмены `rx().await` ещё не проверена на железе.

Это критично для реального чередования RX/TX. Нужен hardware test: начать RX, отменить ожидание передачей, отправить frame, снова принять frame.

## Устаревшее, но исправляется локально

### `Radio1262`

`drivers/src/lora/radio.rs` уже реализует `CspInterface`, но одновременно сохраняет legacy `PhyLayer`, жёстко задаёт SF12, TX 20 dBm, TCXO 1.8 V и DCDC. Он собирается, но дублирует новый adapter.

Миграция обычно локальная: заменить import/constructor на `Sx1262CspInterface` и передать `Sx1262CspConfig`. Старый тип пока оставлен для firmware/examples.

### Clippy старого drivers code

Строгий `cargo clippy -p sat-drivers --no-deps -- -D warnings` блокируют локальные замечания в:

- `flash/ring.rs` и `flash/mod.rs`: `is_multiple_of`, `is_none_or`;
- `sd/sdcard_logger.rs`: упрощение `map_or`, nested `if`, лишний `Ok(...?)`, references;
- `coils`: module inception, слишком много constructor arguments;
- test build `coils/coils.rs`: unused `ComplexField`.

Эти замечания не меняют архитектуру и обычно исправляются несколькими строками. Они не относятся к новым LoRa/storage modules.

## Проверки

Проходят:

```text
cargo check -p sat-drivers
cargo check -p sat-drivers --target thumbv7em-none-eabihf
cargo test -p sat-drivers
```

RAM storage tests проверяют FIFO order, non-destructive `peek`, отсутствие truncation, full queue и replace/remove config values.

Не проходит ожидаемо:

```text
cargo check -p sat-drivers --features legacy-protocol
```

Полный workspace нельзя проверять одной host-командой: firmware packages требуют свои Cortex-M targets/config. Проверять host и firmware packages нужно раздельно.

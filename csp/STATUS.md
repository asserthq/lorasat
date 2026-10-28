# Текущее состояние

Этот файл обновляет агент. Пользователю ничего заранее заполнять не нужно.

## Где остановились

- Этап 1 — формат пакета CSP v2: завершён.
- Работают CSP v2 header, owned packet, encode/decode и передача через очередь памяти.
- Следующий шаг: остановиться до назначения этапа 2; перед ним согласовать Runner, ручки и каналы.

## Настройки проекта

- Библиотека: `csp/`, package `sat-csp`, Rust edition 2024.
- `csp/` добавлен в members и default-members корневого workspace.
- Зависимостей нет; `#![no_std]`, allocator и `unsafe` не используются.
- Эталон: libcsp v2.1, commit `48f7fb0b57f610bf65bab1aa2d1357c3b9722782`.
- Wire-format: `src/csp_id.c`; поля и флаги: `include/csp/csp_types.h` этого commit.

## Принятые решения

- Формат CSP v2, без поддержки v1 на первых этапах.
- Ядро no_std, тесты на компьютере под std.
- Простой код важнее оптимизаций.
- Runner владеет циклом протокола; ручки и механизм взаимодействия согласуем перед этапом 2.
- Библиотека в `csp/` не зависит от других crates workspace: drivers, ядра логики аппарата и firmware.
- Особенности полудуплексного радио учитываем позже на канальном уровне. Текущие `PhyLayer` и LoRa-драйвер не ограничивают этап 1 и при необходимости могут меняться.
- Представление packet: простой owned `Packet<const MTU: usize>` без lifetime; копирование небольшого payload допустимо.
- Wire serialization сосредоточена в `src/wire.rs`; остальной код работает с `Header` и `Packet`.
- Codec вручную повторяет masks/offsets/network byte order libcsp CSP v2; postcard не используется.
- Header — 6 bytes. `Packet<const MTU: usize>` владеет payload; рекомендуемый `DEFAULT_MTU` — 255 bytes целого packet.
- Этап 1 принимает только `Flags::NONE`; CRC32, HMAC, RDP и fragmentation возвращают `UnsupportedFlags` до реализации соответствующего слоя.
- Fixture generator `tools/libcsp_v2_vectors.c` вызывает реальный `csp_id_prepend`; лицензия upstream сохранена в `THIRD_PARTY_LICENSES.md`.

## Последняя проверка

Все проверки после последнего изменения прошли:

- `cargo test -p sat-csp` — 10 integration tests и doc-tests успешно.
- `cargo fmt -p sat-csp -- --check` — успешно.
- `cargo clippy -p sat-csp --all-targets -- -D warnings` — успешно.
- `cargo check -p sat-csp --target thumbv7em-none-eabihf` — успешно.
- C fixture generator собран с `-Wall -Wextra -Werror`; bytes совпадают с Rust tests.

## Открытые вопросы

Для этапа 1 открытых вопросов нет. Каналы, mutex, ожидание, Runner API и взаимодействие с полудуплексным радио согласовать перед этапом 2.

---

Правило обновления: сохранять краткую актуальную выжимку, а не журнал всех сообщений. После каждого законченного подшага и перед переходом в новый чат обновлять прогресс, решения, проверки и следующий шаг. История остаётся в git и чате. Ориентир — до одной страницы, при необходимости немного больше.

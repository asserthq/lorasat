# Local patch of mmc5983_rs 0.0.1

Source: https://github.com/RaulTrombin/MMC5983-rs (crates.io release 0.0.1).
Original MIT license retained in `LICENSE`. The root workspace selects this copy
through `[patch.crates-io]`; do not edit the Cargo registry cache.

## Why this patch exists

Upstream `init()` performs a software reset and immediately writes OTP_READ,
without allowing reset to finish. A delay before `init()` cannot fix a delay
missing inside it. Upstream also caches the self-clearing OTP_READ command and
replays it on later writes to Internal Control 0.

The patch adds `init_with_delay(&mut delay)` (20 ms after reset and OTP_READ,
initial SET pulse, automatic SET/RESET). Legacy `init()` remains available but
is deprecated because its API cannot provide the required timing.

One-shot polling now clears Meas_M_Done, starts exactly one measurement, and
polls without repeatedly triggering it. Each subsequent call starts a new
measurement. `read_magnetic_field()` remains a low-level register read: it does
not start a measurement.

Gauss conversion subtracts the unsigned 18-bit zero-field bias (131072 counts)
before applying 16384 counts/gauss. Calibrated results retain the same biased
representation, avoiding unsigned subtraction underflow.

## Async usage

```rust,ignore
let mut sensor = mmc5983_rs::Mmc5983::new_with_i2c(i2c);
sensor.init_with_delay(&mut embassy_time::Delay).await?;
let field = embassy_time::with_timeout(
    embassy_time::Duration::from_millis(100),
    sensor.magnetic_field(),
).await??;
```

The STM32 example is `examples/stm32f401/src/bin/mmc5983ma.rs`.
Initialization errors and measurement timeouts are logged and retried.
Continuous mode and the full SET/RESET calibration procedure are not validated
by this patch; the example uses one-shot measurements.

## Validation

From the repository root, on the host:

```sh
cargo test --manifest-path vendor/mmc5983_rs/Cargo.toml --target-dir target/mmc5983-tests
```

Tests cover reset/OTP transaction ordering and delays, self-clearing command
caching, one-shot polling, ID validation, signed gauss conversion, and calibrated
field conversion. The synchronous tests exercise the same `maybe_async_cfg`
implementation used by the async build.

For the ARM example, from `examples/stm32f401`:

```sh
cargo check --bin mmc5983ma
```

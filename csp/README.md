# sat-csp

Small `no_std` CSP v2 packet, connectionless datagram, and initial RDP
implementation. It has no allocator, executor, MCU, or workspace-crate
dependencies. `embassy-sync` provides bounded channels and `embassy-futures`
provides executor-independent future selection.

Current scope includes the fixed six-byte CSP v2 header, unreliable single-frame
datagrams, and one libcsp-compatible RDP connection with window 1 and immediate
ACK. CRC32, HMAC, fragmentation, connection close, multiple messages, and
multiple simultaneous connections are not implemented. The whole encoded
packet is limited by const `MTU`; `Packet<255>` accepts 249 datagram payload bytes
or 244 RDP application bytes.

```rust
use sat_csp::{Flags, Header, Packet, Priority};

let header = Header::new(
    Priority::Normal,
    10, // source
    20, // destination
    12, // source port
    13, // destination port
    Flags::NONE,
)?;
let outgoing = Packet::<255>::new(header, b"hello")?;

let mut frame = [0; 255];
let frame_len = outgoing.encode(&mut frame)?;
let incoming = Packet::<255>::decode(&frame[..frame_len])?;

assert_eq!(incoming.header(), header);
assert_eq!(incoming.payload(), b"hello");
# Ok::<(), sat_csp::Error>(())
```

CSP header layout is isolated in `src/wire.rs`; RDP trailer and SYN options are
isolated in `src/rdp.rs`. Protocol-facing code uses `Header` and owned `Packet`
values. Future fragmentation codecs remain separate from their state machines,
so changing an encoder does not spread serialization concerns through the stack.

## Architecture

- `Header` and `Packet` are the owned protocol values.
- `wire` is the CSP v2 header codec.
- `Socket` provides bounded connectionless send/receive queues.
- `Runner` owns one `CspInterface`, packet routing, RX/TX selection, and counters.
- `StackResources` provides datagram queue storage statically.
- `RdpResources` provides one RDP state-machine endpoint and its static queues.
- `CspInterface` is the CSP packet/carrier boundary and transfers complete frames.

The crate does not depend on satellite logic, firmware, or radio-driver crates.
The lower crate implements `CspInterface` for a concrete carrier. `sat-drivers`
currently implements it directly for the LoRa radios. A future MAC can instead
own a radio and implement the same trait, without changes to CSP or applications.

## Datagram stack

Application-owned resources define all queue memory at compile time:

```rust,ignore
let resources = StackResources::<255, 2, 2, 2>::new();
let mut builder = RunnerBuilder::new(local_address, interface, &resources)?;
let mut command_socket = builder.bind(10)?;
let _telemetry_socket = builder.bind(20)?;
let mut runner = builder.build();

// Run this future in an application task.
runner.run().await?;
```

A Socket copies an outgoing payload into an owned packet. `send_to` finishes
when that packet enters the local TX queue, not when radio transmission or
remote delivery completes:

```rust,ignore
command_socket.send_to(remote_address, 30, b"hello").await?;
let datagram = command_socket.recv_from().await;
```

- `try_send_to` returns `SendError::Busy` when the TX queue is full.
- `send_to` waits for local TX capacity.
- A full Socket RX queue drops the newest datagram without blocking Runner.
- Runner drops malformed frames, foreign destinations, and unopened ports.
- Datagram mode never creates ACK or retries lost frames.

## Initial RDP profile

Client setup:

```rust,ignore
let rdp = RdpResources::<255>::new();
rdp.update_time(0);
let mut builder = RunnerBuilder::new(local_address, interface, &stack_resources)?;
let client = builder.rdp_client(&rdp, RdpConfig::new(initial_sequence))?;
let mut runner = builder.build();

// With runner and periodic clock task active:
let mut connection = client.connect(remote_address, command_port).await?;
connection.send(b"command").await?; // returns only after cumulative RDP ACK
```

Server setup:

```rust,ignore
let listener = builder.listen_rdp(
    command_port,
    &rdp,
    RdpConfig::new(initial_sequence),
)?;
let mut connection = listener.accept().await?;
let command = connection.recv().await;
```

Runner must receive wrapping monotonic milliseconds. Start from a known value
before `connect`, then update periodically:

```rust,ignore
rdp.update_time(now_ms);
```

Packet retry defaults follow the pinned libcsp profile: 1000 ms packet timeout
and 10000 ms bounded operation timeout. Lost SYN and DATA are retransmitted.
Lost DATA ACK causes duplicate DATA on wire, but receiver delivers application
data once and repeats the cumulative ACK.

Current deliberate limits:

- one RDP client or listener per Runner;
- fixed outgoing client port 63;
- one accepted connection and one `send` per `RdpConnection` handle;
- `send` confirms queueing at peer RDP receive queue, not command execution;
- no close/RST API and no reuse of a completed connection;
- no EAK/out-of-order buffering;
- cancelling `connect` or `send` does not yet clean protocol state;
- application supplies the initial sequence number and monotonic clock.

`CspInterface::receive` must be cancellation-safe. Runner cancels pending receive
when a queued TX packet must use a half-duplex interface. The next interface
operation must restore the required radio mode.

### Interface errors

Expected per-frame conditions belong inside `CspInterface`. For example, an RX
timeout, invalid radio CRC, or unrelated frame can be discarded before
`receive` returns. `Err` means the interface cannot currently continue: SPI
failure, radio state failure, disconnected host carrier, or similar fault.

Runner returns `RunnerError::Interface` instead of silently retrying because a blind
retry can create a busy loop, duplicate a datagram, or use a radio left in an
unknown mode. The packet being transmitted is already removed from the TX
queue and is considered lost.

The task supervising Runner chooses recovery:

```rust,ignore
loop {
    if let Err(error) = runner.run().await {
        log_interface_error(&error);
        runner.interface_mut().recover().await?; // concrete Interface method
    }
}
```

For a transient `CspInterface` implementation, recovery may be internal and the
next `runner.run()` call may be enough. `interface_mut` exists for
hardware-specific reset or diagnostics while Runner is stopped.

### Radio integration limits

libcsp does not require an interface to be full-duplex and does not provide a
generic receive-while-transmitting guarantee. Each interface driver determines
its own concurrency, queues, and medium access behavior.

The planned vehicle has separate 868 MHz and 435 MHz half-duplex transceivers.
The current Runner owns one interface mutably and alternates software RX/TX
preference. Direct `Radio1262: Interface` therefore performs one radio operation
at a time and does not create a guaranteed RF receive window. A continuously
non-empty TX queue may still delay command reception depending on airtime.

No MAC/window scheduler is implemented yet. A future MAC worker may own the
radio, expose TX/RX queues through `CspInterface`, and enforce receive windows,
duty cycle, backoff, or priority. Fixed command windows, beacon scheduling, and
interleaving a long 435 MHz downlink belong there. SFP fragmentation remains a
separate CSP service. RDP, SFP, HMAC, and CRC32 in libcsp use trailers appended
after application data; each layer removes its own trailer without moving
payload.

`CriticalSectionRawMutex` protects channels. An embedded application must
provide its platform critical-section implementation; host tests use the
`critical-section/std` development feature.

## Examples

Host datagram request/reply over two in-memory links:

```sh
cargo run -p examples-std --bin csp_datagram
```

Host RDP handshake and confirmed command:

```sh
cargo run -p examples-std --bin csp_rdp
```

STM32F401 Embassy compile checks (no real radio):

```sh
cargo check -p examples-stm32f401 --bin csp_datagram \
  --target thumbv7em-none-eabihf
cargo check -p examples-stm32f401 --bin csp_rdp \
  --target thumbv7em-none-eabihf
```

These examples document calling code only. The MCU example does not validate
LoRa mode switching or cancellation safety.

### Three-terminal UDP scenario

Run these commands in separate terminals, preferably satellite first:

```sh
cargo run -p examples-std --bin satellite
```

```sh
cargo run -p examples-std --bin client -- "opaque sensor bytes 42"
```

```sh
cargo run -p examples-std --bin ground
```

The satellite emits the same compact beacon every 10 seconds over two independent
UDP radio mocks. Ground receives it on the 435 MHz mock. Client receives it on
the 868 MHz mock and sends one opaque datagram back. Satellite stores source
metadata plus raw payload without parsing it. Press Enter in ground after client
reports transmission; ground opens RDP, sends `dump`, and receives all records
that fit in one RDP DATA packet.

UDP ports use separate ranges so one satellite process can own both radios:

- 435 MHz mock: `127.0.0.1:(14350 + CSP address)`;
- 868 MHz mock: `127.0.0.1:(18680 + CSP address)`.

This scenario deliberately has no subscriber slots, persistent storage, SFP, or
multi-packet dump. Full source is in `examples/std/src/bin/{ground,satellite,client}.rs`
and shared mock/codecs are in `examples/std/src/csp_demo.rs`.

## Planned order

1. Add close/reset and safe cancellation to the first RDP connection.
2. Later allow several messages per connection and multiple connections.
3. Add SFP fragmentation for payloads larger than one CSP packet.
4. Add separate radio scheduling/MAC policy where hardware requires it.

RDP codec, handshake, one DATA/ACK, retransmission, and bounded timeout are
implemented. No custom OPEN/READY/DONE wire protocol or session identifier is
planned.

## Reference

Wire layout and fixtures follow libcsp v2.1 commit
`48f7fb0b57f610bf65bab1aa2d1357c3b9722782`:

- `src/csp_id.c` for CSP v2 masks, offsets, and network byte order;
- `include/csp/csp_types.h` for fields, priorities, and flags;
- `src/csp_rdp.c` and `src/csp_conn.h` for the planned RDP wire/state reference.

See `tools/README.md` for reproducible fixture generation using the real
libcsp `csp_id_prepend` function.

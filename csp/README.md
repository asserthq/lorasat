# sat-csp

Small `no_std` CSP v2 packet codec. It has no allocator, executor, MCU, or
workspace-crate dependencies.

Current scope is the fixed six-byte CSP v2 header and packets without optional
CRC32, HMAC, RDP, or fragmentation fields. The whole encoded packet is limited
by the const `MTU`; `Packet<255>` therefore accepts payloads up to 249 bytes.

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

All byte layout code is isolated in `src/wire.rs`. Protocol-facing code uses
`Header` and owned `Packet` values.

## Reference

Wire layout and fixtures follow libcsp v2.1 commit
`48f7fb0b57f610bf65bab1aa2d1357c3b9722782`:

- `src/csp_id.c` for CSP v2 masks, offsets, and network byte order;
- `include/csp/csp_types.h` for fields, priorities, and flags.

See `tools/README.md` for reproducible fixture generation using the real
libcsp `csp_id_prepend` function.

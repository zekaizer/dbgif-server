# dbgif-protocol-core

Low-level protocol implementation for DBGIF (Debug Interface) with no_std support.

## Features

- **Zero-copy message parsing** - Streaming decoder for byte-by-byte processing
- **Protocol state machine** - Enforces valid command sequences (CNXN → OPEN → WRTE)
- **Embedded-ready** - Works in no_std environments with static memory
- **C FFI bindings** - Use from C firmware via static library
- **CRC32 validation** - Built-in data integrity checking

## Usage

```rust
use dbgif_protocol_core::*;

// Decode messages
let mut decoder = MessageDecoder::new();
let (result, consumed) = decoder.feed(&bytes);
if result == DecodeResult::Complete {
    let header = decoder.get_header().unwrap();
}

// Encode messages
let mut encoder = MessageEncoder::new();
let data = encoder.encode_with_data(Command::WRTE, arg0, arg1, payload)?;

// Validate protocol state
let mut state = ProtocolStateMachine::new();
state.can_send(Command::CNXN, version, max_payload)?;
state.on_send(Command::CNXN, version, max_payload)?;
```

## For Embedded Systems

```toml
# Cargo.toml
dbgif-protocol-core = { version = "0.1", default-features = false }
```

```c
// C usage
#include "dbgif_protocol.h"

dbgif_decoder_t* decoder = dbgif_decoder_init(buffer, size);
dbgif_decode_result_t result = dbgif_decode_bytes(decoder, data, len, &consumed);
```

## Modules

- `commands` - Protocol command definitions (CNXN, OPEN, WRTE, etc.)
- `decoder` - Streaming message decoder
- `encoder` - Message encoder with automatic CRC
- `state` - Connection and stream state management
- `ffi` - C language bindings

## License

MIT
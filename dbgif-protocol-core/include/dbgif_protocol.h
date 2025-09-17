/**
 * DBGIF Protocol Core - C API
 *
 * This header provides C bindings for the DBGIF protocol decoder
 * with streaming support for embedded systems.
 *
 * Features:
 * - No dynamic memory allocation
 * - Incremental message parsing
 * - Zero-copy data processing
 * - CRC32 validation
 *
 * Usage:
 * 1. Allocate a working buffer (minimum 256 bytes recommended)
 * 2. Initialize decoder with dbgif_decoder_init()
 * 3. Feed data with dbgif_decode_bytes()
 * 4. Check for complete messages
 * 5. Reset decoder for next message
 */

#ifndef DBGIF_PROTOCOL_H
#define DBGIF_PROTOCOL_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Connection States */
#define DBGIF_STATE_DISCONNECTED    0
#define DBGIF_STATE_CONNECTING      1
#define DBGIF_STATE_CONNECTED       2
#define DBGIF_STATE_ERROR           3

/* State Error Codes */
#define DBGIF_ERR_NOT_CONNECTED      10
#define DBGIF_ERR_ALREADY_CONNECTED  11
#define DBGIF_ERR_INVALID_TRANSITION 12
#define DBGIF_ERR_STREAM_NOT_FOUND   13
#define DBGIF_ERR_STREAM_ALREADY_ACTIVE 14
#define DBGIF_ERR_TOO_MANY_STREAMS   15

/* Protocol Commands */
#define DBGIF_CMD_CNXN 0x4E584E43  /* "CNXN" - Connection */
#define DBGIF_CMD_OKAY 0x59414B4F  /* "OKAY" - Success */
#define DBGIF_CMD_OPEN 0x4E45504F  /* "OPEN" - Open stream */
#define DBGIF_CMD_WRTE 0x45545257  /* "WRTE" - Write data */
#define DBGIF_CMD_CLSE 0x45534C43  /* "CLSE" - Close stream */
#define DBGIF_CMD_PING 0x474E4950  /* "PING" - Keep-alive */
#define DBGIF_CMD_PONG 0x474E4F50  /* "PONG" - Keep-alive response */

/* Message header size */
#define DBGIF_HEADER_SIZE 24

/* Decoder states */
typedef enum {
    DBGIF_STATE_WAITING_HEADER = 0,
    DBGIF_STATE_ACCUMULATING_HEADER = 1,
    DBGIF_STATE_WAITING_DATA = 2,
    DBGIF_STATE_ACCUMULATING_DATA = 3,
    DBGIF_STATE_MESSAGE_COMPLETE = 4,
    DBGIF_STATE_ERROR = 5
} dbgif_state_t;

/* Decode results */
typedef enum {
    DBGIF_NEED_MORE_DATA = 0,
    DBGIF_MESSAGE_COMPLETE = 1,
    DBGIF_ERROR_INVALID_MAGIC = -1,
    DBGIF_ERROR_INVALID_COMMAND = -2,
    DBGIF_ERROR_CRC_MISMATCH = -3,
    DBGIF_ERROR_BUFFER_TOO_SMALL = -4,
    DBGIF_ERROR_INVALID_STATE = -5
} dbgif_decode_result_t;

/* Message header structure */
typedef struct {
    uint32_t command;      /* Command code (little-endian) */
    uint32_t arg0;         /* First argument */
    uint32_t arg1;         /* Second argument */
    uint32_t data_length;  /* Data payload length */
    uint32_t data_crc32;   /* CRC32 of data payload */
    uint32_t magic;        /* Magic number (~command) */
} dbgif_header_t;

/* Decoder status */
typedef struct {
    uint32_t state;           /* Current decoder state */
    size_t bytes_needed;      /* Bytes needed to complete current phase */
    size_t bytes_received;    /* Total bytes received for current message */
    uint32_t expected_crc;    /* Expected CRC from header */
    uint32_t current_crc;     /* Currently calculated CRC */
} dbgif_decode_status_t;

/* Opaque decoder handle */
typedef struct DbgifDecoder dbgif_decoder_t;

/* Opaque encoder handle */
typedef struct DbgifEncoder dbgif_encoder_t;

/* Opaque state machine handle */
typedef struct DbgifStateMachine dbgif_state_machine_t;

/**
 * Initialize a decoder instance
 *
 * @param working_buffer Buffer for decoder state and data storage
 * @param buffer_size Size of working buffer (minimum 100 bytes)
 * @return Decoder handle or NULL on failure
 *
 * The working buffer is used as follows:
 * - First sizeof(decoder) bytes: Decoder state
 * - Remaining bytes: Data storage for message payloads
 */
dbgif_decoder_t* dbgif_decoder_init(uint8_t* working_buffer, size_t buffer_size);

/**
 * Reset decoder state for next message
 *
 * @param decoder Decoder instance
 */
void dbgif_decoder_reset(dbgif_decoder_t* decoder);

/**
 * Feed bytes to the streaming decoder
 *
 * @param decoder Decoder instance
 * @param data Input data buffer
 * @param len Length of input data
 * @param consumed Output: number of bytes consumed
 * @return Decode result (see dbgif_decode_result_t)
 *
 * This function can be called with any amount of data,
 * from 1 byte to entire messages. It will consume only
 * what it needs and report back the amount consumed.
 */
dbgif_decode_result_t dbgif_decode_bytes(
    dbgif_decoder_t* decoder,
    const uint8_t* data,
    size_t len,
    size_t* consumed
);

/**
 * Get completed message header and data
 *
 * @param decoder Decoder instance
 * @param header Output: message header
 * @param data Output: pointer to data payload (if any)
 * @param data_len Output: length of data payload
 * @return 0 on success, -1 if no message available
 *
 * The data pointer points to internal buffer and is valid
 * until dbgif_decoder_reset() is called.
 */
int dbgif_get_message(
    dbgif_decoder_t* decoder,
    dbgif_header_t* header,
    const uint8_t** data,
    size_t* data_len
);

/**
 * Get current decoder status
 *
 * @param decoder Decoder instance
 * @param status Output: decoder status
 */
void dbgif_get_status(
    dbgif_decoder_t* decoder,
    dbgif_decode_status_t* status
);

/**
 * Create a message header
 *
 * @param command Command code
 * @param arg0 First argument
 * @param arg1 Second argument
 * @param data_length Data payload length
 * @param data_crc32 CRC32 of data payload
 * @param header Output: message header
 * @return 0 on success, -1 on failure
 */
int dbgif_create_header(
    uint32_t command,
    uint32_t arg0,
    uint32_t arg1,
    uint32_t data_length,
    uint32_t data_crc32,
    dbgif_header_t* header
);

/**
 * Serialize header to byte buffer
 *
 * @param header Message header
 * @param buffer Output buffer (must be at least 24 bytes)
 * @return 0 on success, -1 on failure
 */
int dbgif_serialize_header(
    const dbgif_header_t* header,
    uint8_t* buffer
);

/**
 * Deserialize header from byte buffer
 *
 * @param buffer Input buffer (must be at least 24 bytes)
 * @param header Output: message header
 * @return 0 on success, error code on failure
 */
int dbgif_deserialize_header(
    const uint8_t* buffer,
    dbgif_header_t* header
);

/**
 * Initialize an encoder instance
 *
 * @param working_buffer Buffer for encoder state and output storage
 * @param buffer_size Size of working buffer (minimum 512 bytes recommended)
 * @return Encoder handle or NULL on failure
 *
 * The working buffer is used as follows:
 * - First sizeof(encoder) bytes: Encoder state
 * - Remaining bytes: Output buffer for encoded messages
 */
dbgif_encoder_t* dbgif_encoder_init(uint8_t* working_buffer, size_t buffer_size);

/**
 * Encode a message
 *
 * @param encoder Encoder instance
 * @param command Command code (DBGIF_CMD_*)
 * @param arg0 First argument
 * @param arg1 Second argument
 * @param data Optional data payload (can be NULL)
 * @param data_len Length of data payload
 * @param output Output: pointer to encoded message
 * @param output_len Output: length of encoded message
 * @return 0 on success, negative error code on failure
 *
 * The output pointer points to internal buffer and is valid
 * until dbgif_encoder_clear() is called or another message is encoded.
 */
int dbgif_encode_message(
    dbgif_encoder_t* encoder,
    uint32_t command,
    uint32_t arg0,
    uint32_t arg1,
    const uint8_t* data,
    size_t data_len,
    const uint8_t** output,
    size_t* output_len
);

/**
 * Clear encoder for reuse
 *
 * @param encoder Encoder instance
 */
void dbgif_encoder_clear(dbgif_encoder_t* encoder);

/**
 * Get encoder buffer capacity
 *
 * @param encoder Encoder instance
 * @return Buffer capacity in bytes
 */
size_t dbgif_encoder_capacity(dbgif_encoder_t* encoder);

/**
 * Create a new protocol state machine
 *
 * @return State machine handle or NULL on failure
 *
 * The state machine tracks connection and stream states to ensure
 * protocol compliance and prevent invalid command sequences.
 */
dbgif_state_machine_t* dbgif_state_machine_new(void);

/**
 * Free a state machine instance
 *
 * @param machine State machine instance
 */
void dbgif_state_machine_free(dbgif_state_machine_t* machine);

/**
 * Validate if a command can be sent in current state
 *
 * @param machine State machine instance
 * @param command Command code (DBGIF_CMD_*)
 * @param arg0 First argument
 * @param arg1 Second argument
 * @return 0 if valid, error code otherwise (DBGIF_ERR_*)
 */
int dbgif_state_validate_send(
    dbgif_state_machine_t* machine,
    uint32_t command,
    uint32_t arg0,
    uint32_t arg1
);

/**
 * Update state after sending a command
 *
 * @param machine State machine instance
 * @param command Command code (DBGIF_CMD_*)
 * @param arg0 First argument
 * @param arg1 Second argument
 * @return 0 on success, error code otherwise
 */
int dbgif_state_on_send(
    dbgif_state_machine_t* machine,
    uint32_t command,
    uint32_t arg0,
    uint32_t arg1
);

/**
 * Update state after receiving a command
 *
 * @param machine State machine instance
 * @param command Command code (DBGIF_CMD_*)
 * @param arg0 First argument
 * @param arg1 Second argument
 * @return 0 on success, error code otherwise
 */
int dbgif_state_on_receive(
    dbgif_state_machine_t* machine,
    uint32_t command,
    uint32_t arg0,
    uint32_t arg1
);

/**
 * Get current connection state
 *
 * @param machine State machine instance
 * @return Connection state (DBGIF_STATE_*) or -1 on error
 */
int dbgif_state_get_connection(dbgif_state_machine_t* machine);

/**
 * Check if connected
 *
 * @param machine State machine instance
 * @return 1 if connected, 0 otherwise
 */
int dbgif_state_is_connected(dbgif_state_machine_t* machine);

/**
 * Reset state machine to initial state
 *
 * @param machine State machine instance
 */
void dbgif_state_reset(dbgif_state_machine_t* machine);

/* Example usage for embedded systems:
 *
 * // Decoder example
 * static uint8_t decoder_buffer[256];
 * static dbgif_decoder_t* decoder;
 *
 * void protocol_init(void) {
 *     decoder = dbgif_decoder_init(decoder_buffer, sizeof(decoder_buffer));
 * }
 *
 * void uart_rx_handler(uint8_t byte) {
 *     size_t consumed;
 *     dbgif_decode_result_t result = dbgif_decode_bytes(decoder, &byte, 1, &consumed);
 *
 *     if (result == DBGIF_MESSAGE_COMPLETE) {
 *         dbgif_header_t header;
 *         const uint8_t* data;
 *         size_t data_len;
 *
 *         if (dbgif_get_message(decoder, &header, &data, &data_len) == 0) {
 *             handle_message(&header, data, data_len);
 *         }
 *
 *         dbgif_decoder_reset(decoder);
 *     }
 * }
 *
 * // Encoder example
 * static uint8_t encoder_buffer[512];
 * static dbgif_encoder_t* encoder;
 *
 * void send_ping(void) {
 *     const uint8_t* output;
 *     size_t output_len;
 *
 *     if (dbgif_encode_message(encoder, DBGIF_CMD_PING, 0, 0, NULL, 0, &output, &output_len) == 0) {
 *         uart_send(output, output_len);
 *     }
 * }
 *
 * void send_data(uint32_t local_id, uint32_t remote_id, const uint8_t* data, size_t len) {
 *     const uint8_t* output;
 *     size_t output_len;
 *
 *     if (dbgif_encode_message(encoder, DBGIF_CMD_WRTE, local_id, remote_id, data, len, &output, &output_len) == 0) {
 *         uart_send(output, output_len);
 *     }
 *
 *     dbgif_encoder_clear(encoder);  // Ready for next message
 * }
 *
 * // State machine example
 * static dbgif_state_machine_t* state;
 *
 * void protocol_init(void) {
 *     state = dbgif_state_machine_new();
 * }
 *
 * void send_command(uint32_t cmd, uint32_t arg0, uint32_t arg1) {
 *     // Validate before sending
 *     if (dbgif_state_validate_send(state, cmd, arg0, arg1) == 0) {
 *         // Send the command
 *         send_to_device(cmd, arg0, arg1);
 *         // Update state
 *         dbgif_state_on_send(state, cmd, arg0, arg1);
 *     }
 * }
 *
 * void receive_command(uint32_t cmd, uint32_t arg0, uint32_t arg1) {
 *     // Update state based on received command
 *     dbgif_state_on_receive(state, cmd, arg0, arg1);
 * }
 */

#ifdef __cplusplus
}
#endif

#endif /* DBGIF_PROTOCOL_H */
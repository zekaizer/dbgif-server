/**
 * Example usage of DBGIF protocol parser in embedded C
 *
 * This example demonstrates how to use the streaming parser
 * in a typical embedded system with UART communication.
 */

#include "dbgif_protocol.h"
#include <stdio.h>
#include <string.h>

/* Working buffer for parser */
static uint8_t parser_buffer[512];
static dbgif_parser_t* g_parser = NULL;

/* UART receive buffer */
static uint8_t uart_rx_buffer[64];
static size_t uart_rx_len = 0;

/* Initialize protocol handler */
void protocol_init(void) {
    g_parser = dbgif_parser_init(parser_buffer, sizeof(parser_buffer));
    if (g_parser == NULL) {
        printf("Failed to initialize parser\n");
    }
}

/* Handle completed message */
void handle_message(const dbgif_header_t* header, const uint8_t* data, size_t data_len) {
    printf("Received message: cmd=0x%08X, arg0=%u, arg1=%u, len=%zu\n",
           header->command, header->arg0, header->arg1, data_len);

    switch (header->command) {
        case DBGIF_CMD_CNXN:
            printf("Connection request received\n");
            /* Send CNXN response */
            send_cnxn_response();
            break;

        case DBGIF_CMD_OPEN:
            if (data_len > 0) {
                printf("Open stream request: %.*s\n", (int)data_len, data);
            }
            /* Send OKAY response */
            send_okay_response(header->arg0);
            break;

        case DBGIF_CMD_WRTE:
            if (data_len > 0) {
                printf("Data received: %.*s\n", (int)data_len, data);
            }
            /* Process data and send OKAY */
            process_write_data(data, data_len);
            send_okay_response(header->arg0);
            break;

        case DBGIF_CMD_PING:
            /* Send PONG response */
            send_pong_response(header->arg0, header->arg1);
            break;

        default:
            printf("Unknown command: 0x%08X\n", header->command);
            break;
    }
}

/* UART receive interrupt handler */
void uart_rx_interrupt(void) {
    /* Read available bytes from UART */
    uart_rx_len = uart_read(uart_rx_buffer, sizeof(uart_rx_buffer));

    size_t offset = 0;
    while (offset < uart_rx_len) {
        size_t consumed = 0;
        dbgif_parse_result_t result = dbgif_feed_bytes(
            g_parser,
            uart_rx_buffer + offset,
            uart_rx_len - offset,
            &consumed
        );

        offset += consumed;

        if (result == DBGIF_MESSAGE_COMPLETE) {
            dbgif_header_t header;
            const uint8_t* data;
            size_t data_len;

            if (dbgif_get_message(g_parser, &header, &data, &data_len) == 0) {
                handle_message(&header, data, data_len);
            }

            /* Reset parser for next message */
            dbgif_parser_reset(g_parser);

        } else if (result < 0) {
            printf("Parse error: %d\n", result);
            /* Reset parser on error */
            dbgif_parser_reset(g_parser);
            break;
        }
        /* else DBGIF_NEED_MORE_DATA - continue accumulating */
    }
}

/* Send CNXN response */
void send_cnxn_response(void) {
    const char* identity = "device:embedded:v1.0";
    uint32_t crc = dbgif_calculate_crc32((const uint8_t*)identity, strlen(identity));

    dbgif_header_t header;
    dbgif_create_header(
        DBGIF_CMD_CNXN,
        0x01000000,  /* version */
        256 * 1024,  /* max data */
        strlen(identity),
        crc,
        &header
    );

    uint8_t header_buf[DBGIF_HEADER_SIZE];
    dbgif_serialize_header(&header, header_buf);

    /* Send header + data */
    uart_write(header_buf, sizeof(header_buf));
    uart_write((const uint8_t*)identity, strlen(identity));
}

/* Send OKAY response */
void send_okay_response(uint32_t remote_id) {
    static uint32_t local_id = 100;

    dbgif_header_t header;
    dbgif_create_header(
        DBGIF_CMD_OKAY,
        local_id++,
        remote_id,
        0,
        0,
        &header
    );

    uint8_t header_buf[DBGIF_HEADER_SIZE];
    dbgif_serialize_header(&header, header_buf);
    uart_write(header_buf, sizeof(header_buf));
}

/* Send PONG response */
void send_pong_response(uint32_t connect_id, uint32_t token) {
    dbgif_header_t header;
    dbgif_create_header(
        DBGIF_CMD_PONG,
        connect_id,
        token,
        0,
        0,
        &header
    );

    uint8_t header_buf[DBGIF_HEADER_SIZE];
    dbgif_serialize_header(&header, header_buf);
    uart_write(header_buf, sizeof(header_buf));
}

/* Process write data (example) */
void process_write_data(const uint8_t* data, size_t len) {
    /* Example: Echo back the data */
    dbgif_header_t header;
    uint32_t crc = dbgif_calculate_crc32(data, len);

    dbgif_create_header(
        DBGIF_CMD_WRTE,
        100,  /* local_id */
        1,    /* remote_id */
        len,
        crc,
        &header
    );

    uint8_t header_buf[DBGIF_HEADER_SIZE];
    dbgif_serialize_header(&header, header_buf);

    uart_write(header_buf, sizeof(header_buf));
    uart_write(data, len);
}

/* Main function for testing */
int main(void) {
    protocol_init();

    /* Simulate receiving a PING message byte by byte */
    dbgif_header_t ping_header;
    dbgif_create_header(DBGIF_CMD_PING, 0x12345678, 0xABCDEF00, 0, 0, &ping_header);

    uint8_t ping_buffer[DBGIF_HEADER_SIZE];
    dbgif_serialize_header(&ping_header, ping_buffer);

    printf("Simulating PING message reception...\n");
    for (size_t i = 0; i < DBGIF_HEADER_SIZE; i++) {
        uart_rx_buffer[0] = ping_buffer[i];
        uart_rx_len = 1;
        uart_rx_interrupt();
    }

    return 0;
}
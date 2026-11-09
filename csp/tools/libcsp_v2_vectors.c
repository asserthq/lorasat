#if defined(__has_include)
#if __has_include(<csp/csp.h>) && __has_include(<csp/csp_id.h>)
#define HAVE_LIBCSP_HEADERS 1
#endif
#endif

#if HAVE_LIBCSP_HEADERS

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <csp/csp.h>
#include <csp/csp_id.h>

static void print_vector(const char *name, uint8_t priority, uint16_t source,
                         uint16_t destination, uint8_t source_port,
                         uint8_t destination_port, uint8_t flags,
                         const uint8_t *payload, uint16_t payload_length) {
    csp_packet_t packet;
    memset(&packet, 0, sizeof(packet));

    packet.id.pri = priority;
    packet.id.src = source;
    packet.id.dst = destination;
    packet.id.sport = source_port;
    packet.id.dport = destination_port;
    packet.id.flags = flags;
    packet.length = payload_length;
    memcpy(packet.data, payload, payload_length);

    csp_conf.version = 2;
    csp_id_prepend(&packet);

    printf("%s:", name);
    for (uint16_t index = 0; index < packet.frame_length; ++index) {
        printf(" %02X", packet.frame_begin[index]);
    }
    putchar('\n');
}

int main(void) {
    static const uint8_t empty[] = {0};
    static const uint8_t hello[] = {'h', 'e', 'l', 'l', 'o'};

    print_vector("zero", 0, 0, 0, 0, 0, 0, empty, 0);
    print_vector("distinct", 2, 0x1234, 0x2345, 0x15, 0x2A, 0, empty, 0);
    print_vector("direction", 1, 2, 1, 4, 3, 0, empty, 0);
    print_vector("maximum", 3, 0x3FFF, 0x3FFF, 0x3F, 0x3F, 0x3F, empty, 0);
    print_vector("packet", 2, 0x1234, 0x2345, 0x15, 0x2A, 0, hello, sizeof(hello));
    return 0;
}

#else

int main(void) {
    return 1;
}

#endif

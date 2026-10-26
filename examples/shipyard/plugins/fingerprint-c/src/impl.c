#include "fingerprint.h"
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static uint64_t fnv1a(const uint8_t *data, size_t len) {
    uint64_t hash = 1469598103934665603ull;
    for (size_t i = 0; i < len; i++) {
        hash ^= data[i];
        hash *= 1099511628211ull;
    }
    return hash;
}

void exports_shipyard_fingerprint_api_fingerprint(fingerprint_string_t *input, fingerprint_string_t *ret) {
    char buf[40];
    uint64_t hash = fnv1a(input->ptr, input->len);
    snprintf(buf, sizeof(buf), "fnv1a-%016llx", (unsigned long long)hash);
    fingerprint_string_dup(ret, buf);
}

bool exports_shipyard_fingerprint_api_verify(fingerprint_string_t *input, fingerprint_string_t *expected) {
    fingerprint_string_t actual;
    exports_shipyard_fingerprint_api_fingerprint(input, &actual);
    bool ok = actual.len == expected->len && memcmp(actual.ptr, expected->ptr, actual.len) == 0;
    fingerprint_string_free(&actual);
    return ok;
}

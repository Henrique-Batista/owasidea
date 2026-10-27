#include "{{name}}.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void exports_poc_{{name}}_api_greet({{name}}_string_t *name, {{name}}_string_t *ret) {
    size_t len = 7 + name->len + 11 + 1;
    char *buf = (char*)malloc(len);
    snprintf(buf, len, "Hello, %.*s! (from C)", (int)name->len, (char*)name->ptr);
    {{name}}_string_dup(ret, buf);
    free(buf);
}

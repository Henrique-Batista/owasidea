#include "{{ident}}.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// owasidea:world {{name}}
// owasidea:export
char *greet(const char *name);

void exports_poc_{{ident}}_api_greet({{ident}}_string_t *name, {{ident}}_string_t *ret) {
    size_t len = 7 + name->len + 11 + 1;
    char *buf = (char*)malloc(len);
    snprintf(buf, len, "Hello, %.*s! (from C)", (int)name->len, (char*)name->ptr);
    {{ident}}_string_dup(ret, buf);
    free(buf);
}

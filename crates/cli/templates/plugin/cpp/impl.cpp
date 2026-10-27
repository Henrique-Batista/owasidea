#include "{{name}}_cpp.h"
#include <cstring>
#include <cstdlib>

namespace exports::poc::{{name}}::api {
    wit::string Greet(wit::string name) {
        const char* prefix = "Hello, ";
        const char* suffix = "! (from C++)";
        size_t len = 7 + name.size() + 12;
        char* buf = (char*)malloc(len + 1);
        memcpy(buf, prefix, 7);
        memcpy(buf + 7, name.data(), name.size());
        memcpy(buf + 7 + name.size(), suffix, 12);
        buf[len] = '\0';
        auto result = wit::string(buf, len);
        return result;
    }
}

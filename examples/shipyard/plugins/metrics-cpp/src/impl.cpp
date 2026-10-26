#include "metrics_cpp.h"
#include <cstdio>
#include <string>
#include <string_view>

namespace exports::shipyard::metrics::api {

wit::string Analyze(wit::string text) {
    std::string value = text.to_string();
    size_t words = 0;
    size_t lines = value.empty() ? 0 : 1;
    bool in_word = false;
    for (char ch : value) {
        if (ch == '\n') lines++;
        if (ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r') {
            in_word = false;
        } else if (!in_word) {
            in_word = true;
            words++;
        }
    }
    size_t minutes = (words + 199) / 200;
    char out[160];
    std::snprintf(out, sizeof(out),
        "{\"characters\":%zu,\"words\":%zu,\"lines\":%zu,\"reading_minutes\":%zu}",
        value.size(), words, lines, minutes);
    return wit::string::from_view(std::string_view(out));
}

}

#pragma once

#include <windows.h>

#include <cstddef>
#include <cstdint>
#include <cstdio>

namespace neptune_test {

class Timer {
public:
  Timer() {
    QueryPerformanceFrequency(&frequency_);
    QueryPerformanceCounter(&started_);
  }

  double elapsed_ms() const {
    LARGE_INTEGER now;
    QueryPerformanceCounter(&now);
    return 1000.0 * static_cast<double>(now.QuadPart - started_.QuadPart) /
           static_cast<double>(frequency_.QuadPart);
  }

private:
  LARGE_INTEGER frequency_;
  LARGE_INTEGER started_;
};

inline std::uint64_t checksum(const void *data, std::size_t size) {
  const auto *bytes = static_cast<const std::uint8_t *>(data);
  std::uint64_t value = 14695981039346656037ull;
  for (std::size_t i = 0; i < size; ++i) {
    value ^= bytes[i];
    value *= 1099511628211ull;
  }
  return value;
}

inline void print_success(const char *name, const char *api, double elapsed_ms,
                          std::uint64_t value) {
  char json[256];
  std::snprintf(json, sizeof(json),
                "{\"test\":\"%s\",\"api\":\"%s\",\"elapsed_ms\":%.3f,"
                "\"checksum\":\"0x%016llx\",\"status\":\"pass\"}\n",
                name, api, elapsed_ms, static_cast<unsigned long long>(value));
  OutputDebugStringA(json);
  std::printf("%s", json);
  std::fflush(stdout);
}

} // namespace neptune_test

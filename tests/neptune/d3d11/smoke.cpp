#include "result.hpp"

#include <d3d11.h>

#include <cstddef>
#include <cstdint>
#include <cstdio>

namespace {

constexpr std::size_t WORD_COUNT = 256;

std::uint32_t expected_word(std::size_t index) {
  return 0xa5a5a5a5u ^ (0x9e3779b9u * static_cast<std::uint32_t>(index + 1));
}

} // namespace

int main() {
  neptune_test::Timer timer;
  ID3D11Device *device = nullptr;
  ID3D11DeviceContext *context = nullptr;
  ID3D11Buffer *gpu_buffer = nullptr;
  ID3D11Buffer *readback_buffer = nullptr;
  D3D_FEATURE_LEVEL feature_level = D3D_FEATURE_LEVEL_11_0;
  D3D11_BUFFER_DESC gpu_desc = {};
  D3D11_BUFFER_DESC readback_desc = {};
  D3D11_SUBRESOURCE_DATA initial_data = {};
  D3D11_MAPPED_SUBRESOURCE mapped = {};
  std::uint32_t input[WORD_COUNT] = {};
  HRESULT result = S_OK;
  int exit_code = 1;

  for (std::size_t i = 0; i < WORD_COUNT; ++i) {
    input[i] = expected_word(i);
  }

  result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0,
                             &feature_level, 1, D3D11_SDK_VERSION, &device,
                             nullptr, &context);
  if (FAILED(result)) {
    std::fprintf(stderr, "D3D11CreateDevice failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  gpu_desc.ByteWidth = sizeof(input);
  gpu_desc.Usage = D3D11_USAGE_DEFAULT;
  initial_data.pSysMem = input;
  result = device->CreateBuffer(&gpu_desc, &initial_data, &gpu_buffer);
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateBuffer(default) failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  readback_desc.ByteWidth = sizeof(input);
  readback_desc.Usage = D3D11_USAGE_STAGING;
  readback_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
  result = device->CreateBuffer(&readback_desc, nullptr, &readback_buffer);
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateBuffer(staging) failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  context->CopyResource(readback_buffer, gpu_buffer);
  result = context->Map(readback_buffer, 0, D3D11_MAP_READ, 0, &mapped);
  if (FAILED(result)) {
    std::fprintf(stderr, "Map failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  for (std::size_t i = 0; i < WORD_COUNT; ++i) {
    const auto *words = static_cast<const std::uint32_t *>(mapped.pData);
    if (words[i] != expected_word(i)) {
      std::fprintf(stderr, "incorrect value at %zu: 0x%08x\n", i, words[i]);
      context->Unmap(readback_buffer, 0);
      goto cleanup;
    }
  }

  neptune_test::print_success(
      "smoke", "d3d11", timer.elapsed_ms(),
      neptune_test::checksum(mapped.pData, sizeof(input)));
  context->Unmap(readback_buffer, 0);
  exit_code = 0;

cleanup:
  if (readback_buffer != nullptr) {
    readback_buffer->Release();
  }
  if (gpu_buffer != nullptr) {
    gpu_buffer->Release();
  }
  if (context != nullptr) {
    context->Release();
  }
  if (device != nullptr) {
    device->Release();
  }
  return exit_code;
}

#include "result.hpp"

#include <d3d11_4.h>

#include <cstdint>
#include <cstdio>

int main() {
  constexpr UINT BUFFER_SIZE = 64u << 20;
  constexpr int ITERATIONS = 500;

  neptune_test::Timer timer;
  ID3D11Device *base_device = nullptr;
  ID3D11Device5 *device = nullptr;
  ID3D11DeviceContext *base_context = nullptr;
  ID3D11DeviceContext4 *context = nullptr;
  ID3D11Buffer *source = nullptr;
  ID3D11Buffer *destination = nullptr;
  ID3D11Fence *fence = nullptr;
  HANDLE event = nullptr;
  D3D_FEATURE_LEVEL feature_level = D3D_FEATURE_LEVEL_11_0;
  D3D11_BUFFER_DESC buffer_desc = {};
  int stale = 0;
  int armed = 0;
  UINT64 worst_gap = 0;
  HRESULT result = S_OK;
  int exit_code = 1;

  result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, 0,
                             &feature_level, 1, D3D11_SDK_VERSION, &base_device,
                             nullptr, &base_context);
  if (FAILED(result)) {
    std::fprintf(stderr, "D3D11CreateDevice failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }
  result = base_device->QueryInterface(IID_PPV_ARGS(&device));
  if (SUCCEEDED(result)) {
    result = base_context->QueryInterface(IID_PPV_ARGS(&context));
  }
  if (FAILED(result)) {
    std::fprintf(stderr, "D3D11 fence interfaces unavailable: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  buffer_desc.ByteWidth = BUFFER_SIZE;
  buffer_desc.Usage = D3D11_USAGE_DEFAULT;
  result = device->CreateBuffer(&buffer_desc, nullptr, &source);
  if (SUCCEEDED(result)) {
    result = device->CreateBuffer(&buffer_desc, nullptr, &destination);
  }
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateBuffer failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  result = device->CreateFence(0, D3D11_FENCE_FLAG_NONE,
                               IID_PPV_ARGS(&fence));
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateFence failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }
  event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
  if (event == nullptr) {
    std::fprintf(stderr, "CreateEventW failed: %lu\n",
                 static_cast<unsigned long>(GetLastError()));
    goto cleanup;
  }

  for (int i = 1; i <= ITERATIONS; ++i) {
    const UINT64 value = static_cast<UINT64>(i);
    context->CopyResource(destination, source);
    result = context->Signal(fence, value);
    if (FAILED(result)) {
      std::fprintf(stderr, "Signal failed: 0x%08lx\n",
                   static_cast<unsigned long>(result));
      goto cleanup;
    }
    if (fence->GetCompletedValue() >= value) {
      continue;
    }
    ++armed;
    result = fence->SetEventOnCompletion(value, event);
    if (FAILED(result)) {
      std::fprintf(stderr, "SetEventOnCompletion failed: 0x%08lx\n",
                   static_cast<unsigned long>(result));
      goto cleanup;
    }
    if (WaitForSingleObject(event, 10000) != WAIT_OBJECT_0) {
      std::fprintf(stderr, "fence event for %llu never fired\n",
                   static_cast<unsigned long long>(value));
      goto cleanup;
    }
    const UINT64 completed = fence->GetCompletedValue();
    if (completed < value) {
      ++stale;
      if (value - completed > worst_gap) {
        worst_gap = value - completed;
      }
    }
  }

  {
    char line[160];
    std::snprintf(line, sizeof(line),
                  "fence_event: %d waits armed, %d saw a stale "
                  "GetCompletedValue after the event (worst gap %llu)\n",
                  armed, stale, static_cast<unsigned long long>(worst_gap));
    OutputDebugStringA(line);
  }
  if (armed == 0) {
    std::fprintf(stderr, "no wait was armed; the test exercised nothing\n");
    goto cleanup;
  }
  if (stale != 0) {
    goto cleanup;
  }

  neptune_test::print_success("fence_event", "d3d11", timer.elapsed_ms(),
                              static_cast<std::uint64_t>(armed));
  exit_code = 0;

cleanup:
  if (event != nullptr) {
    CloseHandle(event);
  }
  if (fence != nullptr) {
    fence->Release();
  }
  if (destination != nullptr) {
    destination->Release();
  }
  if (source != nullptr) {
    source->Release();
  }
  if (context != nullptr) {
    context->Release();
  }
  if (base_context != nullptr) {
    base_context->Release();
  }
  if (device != nullptr) {
    device->Release();
  }
  if (base_device != nullptr) {
    base_device->Release();
  }
  return exit_code;
}

#include "result.hpp"

#include <d3d12.h>

#include <cstdint>
#include <cstdio>

// Once SetEventOnCompletion(V, event) has signalled the event,
// GetCompletedValue() must already report at least V. Engines that arm a
// value once and then re-check the fence after the wake (UE5's RHI interrupt
// thread) stall until their next timeout when it does not.
int main() {
  constexpr UINT64 BUFFER_SIZE = 64ull << 20;
  constexpr int ITERATIONS = 500;

  neptune_test::Timer timer;
  ID3D12Device *device = nullptr;
  ID3D12CommandQueue *queue = nullptr;
  ID3D12Resource *source = nullptr;
  ID3D12Resource *destination = nullptr;
  ID3D12CommandAllocator *allocator = nullptr;
  ID3D12GraphicsCommandList *command_list = nullptr;
  ID3D12Fence *fence = nullptr;
  HANDLE event = nullptr;
  D3D12_COMMAND_QUEUE_DESC queue_desc = {};
  D3D12_HEAP_PROPERTIES heap = {};
  D3D12_RESOURCE_DESC buffer_desc = {};
  ID3D12CommandList *command_lists[1] = {};
  int stale = 0;
  int armed = 0;
  UINT64 worst_gap = 0;
  HRESULT result = S_OK;
  int exit_code = 1;

  result =
      D3D12CreateDevice(nullptr, D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device));
  if (FAILED(result)) {
    std::fprintf(stderr, "D3D12CreateDevice failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  queue_desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
  result = device->CreateCommandQueue(&queue_desc, IID_PPV_ARGS(&queue));
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateCommandQueue failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  heap.Type = D3D12_HEAP_TYPE_DEFAULT;
  buffer_desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
  buffer_desc.Width = BUFFER_SIZE;
  buffer_desc.Height = 1;
  buffer_desc.DepthOrArraySize = 1;
  buffer_desc.MipLevels = 1;
  buffer_desc.SampleDesc.Count = 1;
  buffer_desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
  result = device->CreateCommittedResource(
      &heap, D3D12_HEAP_FLAG_NONE, &buffer_desc, D3D12_RESOURCE_STATE_COMMON,
      nullptr, IID_PPV_ARGS(&source));
  if (SUCCEEDED(result)) {
    result = device->CreateCommittedResource(
        &heap, D3D12_HEAP_FLAG_NONE, &buffer_desc,
        D3D12_RESOURCE_STATE_COMMON, nullptr, IID_PPV_ARGS(&destination));
  }
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateCommittedResource failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  result = device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,
                                          IID_PPV_ARGS(&allocator));
  if (SUCCEEDED(result)) {
    result =
        device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator,
                                  nullptr, IID_PPV_ARGS(&command_list));
  }
  if (FAILED(result)) {
    std::fprintf(stderr, "command list creation failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }
  // Buffers in COMMON promote to COPY_SOURCE/COPY_DEST and decay back at
  // the end of each ExecuteCommandLists, so one list can be replayed.
  command_list->CopyBufferRegion(destination, 0, source, 0, BUFFER_SIZE);
  result = command_list->Close();
  if (FAILED(result)) {
    std::fprintf(stderr, "Close failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }
  command_lists[0] = command_list;

  result = device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence));
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
    queue->ExecuteCommandLists(1, command_lists);
    result = queue->Signal(fence, value);
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

  neptune_test::print_success("fence_event", "d3d12", timer.elapsed_ms(),
                              static_cast<std::uint64_t>(armed));
  exit_code = 0;

cleanup:
  if (event != nullptr) {
    CloseHandle(event);
  }
  if (fence != nullptr) {
    fence->Release();
  }
  if (command_list != nullptr) {
    command_list->Release();
  }
  if (allocator != nullptr) {
    allocator->Release();
  }
  if (destination != nullptr) {
    destination->Release();
  }
  if (source != nullptr) {
    source->Release();
  }
  if (queue != nullptr) {
    queue->Release();
  }
  if (device != nullptr) {
    device->Release();
  }
  return exit_code;
}

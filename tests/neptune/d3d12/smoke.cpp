#include "result.hpp"

#include <d3d12.h>

#include <cstddef>
#include <cstdint>
#include <cstdio>

namespace {

constexpr std::size_t WORD_COUNT = 256;

std::uint32_t expected_word(std::size_t index) {
  return 0x3c6ef372u + (0x85ebca6bu * static_cast<std::uint32_t>(index));
}

} // namespace

int main() {
  neptune_test::Timer timer;
  ID3D12Device *device = nullptr;
  ID3D12CommandQueue *queue = nullptr;
  ID3D12Resource *upload_buffer = nullptr;
  ID3D12Resource *readback_buffer = nullptr;
  ID3D12CommandAllocator *allocator = nullptr;
  ID3D12GraphicsCommandList *command_list = nullptr;
  ID3D12Fence *fence = nullptr;
  HANDLE fence_event = nullptr;
  D3D12_COMMAND_QUEUE_DESC queue_desc = {};
  D3D12_HEAP_PROPERTIES upload_heap = {};
  D3D12_HEAP_PROPERTIES readback_heap = {};
  D3D12_RESOURCE_DESC buffer_desc = {};
  D3D12_RANGE read_range = {0, sizeof(std::uint32_t) * WORD_COUNT};
  D3D12_RANGE written_range = {0, 0};
  ID3D12CommandList *command_lists[1] = {};
  std::uint32_t *upload_data = nullptr;
  void *readback_mapping = nullptr;
  const std::uint32_t *readback_data = nullptr;
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

  upload_heap.Type = D3D12_HEAP_TYPE_UPLOAD;
  upload_heap.CreationNodeMask = 1;
  upload_heap.VisibleNodeMask = 1;
  readback_heap.Type = D3D12_HEAP_TYPE_READBACK;
  readback_heap.CreationNodeMask = 1;
  readback_heap.VisibleNodeMask = 1;

  buffer_desc.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
  buffer_desc.Width = sizeof(std::uint32_t) * WORD_COUNT;
  buffer_desc.Height = 1;
  buffer_desc.DepthOrArraySize = 1;
  buffer_desc.MipLevels = 1;
  buffer_desc.SampleDesc.Count = 1;
  buffer_desc.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;

  result = device->CreateCommittedResource(
      &upload_heap, D3D12_HEAP_FLAG_NONE, &buffer_desc,
      D3D12_RESOURCE_STATE_GENERIC_READ, nullptr, IID_PPV_ARGS(&upload_buffer));
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateCommittedResource(upload) failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  result = device->CreateCommittedResource(
      &readback_heap, D3D12_HEAP_FLAG_NONE, &buffer_desc,
      D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&readback_buffer));
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateCommittedResource(readback) failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  result =
      upload_buffer->Map(0, nullptr, reinterpret_cast<void **>(&upload_data));
  if (FAILED(result)) {
    std::fprintf(stderr, "Map(upload) failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }
  for (std::size_t i = 0; i < WORD_COUNT; ++i) {
    upload_data[i] = expected_word(i);
  }
  upload_buffer->Unmap(0, nullptr);
  upload_data = nullptr;

  result = device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,
                                          IID_PPV_ARGS(&allocator));
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateCommandAllocator failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  result =
      device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator,
                                nullptr, IID_PPV_ARGS(&command_list));
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateCommandList failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  command_list->CopyBufferRegion(readback_buffer, 0, upload_buffer, 0,
                                 buffer_desc.Width);
  result = command_list->Close();
  if (FAILED(result)) {
    std::fprintf(stderr, "Close(command list) failed: 0x%08lx\n",
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

  fence_event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
  if (fence_event == nullptr) {
    std::fprintf(stderr, "CreateEventW failed: %lu\n",
                 static_cast<unsigned long>(GetLastError()));
    goto cleanup;
  }

  queue->ExecuteCommandLists(1, command_lists);
  result = queue->Signal(fence, 1);
  if (FAILED(result)) {
    std::fprintf(stderr, "Signal failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }
  if (fence->GetCompletedValue() < 1) {
    result = fence->SetEventOnCompletion(1, fence_event);
    if (FAILED(result)) {
      std::fprintf(stderr, "SetEventOnCompletion failed: 0x%08lx\n",
                   static_cast<unsigned long>(result));
      goto cleanup;
    }
    if (WaitForSingleObject(fence_event, INFINITE) != WAIT_OBJECT_0) {
      std::fprintf(stderr, "fence wait failed: %lu\n",
                   static_cast<unsigned long>(GetLastError()));
      goto cleanup;
    }
  }

  result = readback_buffer->Map(0, &read_range, &readback_mapping);
  if (FAILED(result)) {
    std::fprintf(stderr, "Map(readback) failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }
  readback_data = static_cast<const std::uint32_t *>(readback_mapping);

  for (std::size_t i = 0; i < WORD_COUNT; ++i) {
    if (readback_data[i] != expected_word(i)) {
      std::fprintf(stderr, "incorrect value at %zu: 0x%08x\n", i,
                   readback_data[i]);
      goto cleanup;
    }
  }

  neptune_test::print_success(
      "smoke", "d3d12", timer.elapsed_ms(),
      neptune_test::checksum(readback_data,
                             sizeof(std::uint32_t) * WORD_COUNT));
  exit_code = 0;

cleanup:
  if (readback_data != nullptr) {
    readback_buffer->Unmap(0, &written_range);
  }
  if (fence_event != nullptr) {
    CloseHandle(fence_event);
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
  if (readback_buffer != nullptr) {
    readback_buffer->Release();
  }
  if (upload_buffer != nullptr) {
    upload_buffer->Release();
  }
  if (queue != nullptr) {
    queue->Release();
  }
  if (device != nullptr) {
    device->Release();
  }
  return exit_code;
}

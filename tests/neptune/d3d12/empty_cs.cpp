#include "result.hpp"

#include <d3d12.h>

#include <cstdint>
#include <cstdio>

// A compute pipeline with an empty CS must fail at the call, with no
// pipeline returned, and leave the device working. Black Myth: Wukong
// creates one and then names it; the name forced the host compile, which
// crashed D3DMetal.
int main() {
  neptune_test::Timer timer;
  ID3D12Device *device = nullptr;
  ID3DBlob *root_blob = nullptr;
  ID3D12RootSignature *root_signature = nullptr;
  ID3D12PipelineState *pipeline = nullptr;
  ID3D12CommandQueue *queue = nullptr;
  ID3D12Fence *fence = nullptr;
  HANDLE fence_event = nullptr;
  D3D12_ROOT_SIGNATURE_DESC root_desc = {};
  D3D12_COMPUTE_PIPELINE_STATE_DESC pipeline_desc = {};
  D3D12_COMMAND_QUEUE_DESC queue_desc = {};
  HRESULT create_result = S_OK;
  HRESULT result = S_OK;
  int exit_code = 1;

  result =
      D3D12CreateDevice(nullptr, D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device));
  if (FAILED(result)) {
    std::fprintf(stderr, "D3D12CreateDevice failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  result = D3D12SerializeRootSignature(&root_desc, D3D_ROOT_SIGNATURE_VERSION_1,
                                       &root_blob, nullptr);
  if (FAILED(result)) {
    std::fprintf(stderr, "D3D12SerializeRootSignature failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }
  result = device->CreateRootSignature(0, root_blob->GetBufferPointer(),
                                       root_blob->GetBufferSize(),
                                       IID_PPV_ARGS(&root_signature));
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateRootSignature failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }

  pipeline_desc.pRootSignature = root_signature;
  create_result =
      device->CreateComputePipelineState(&pipeline_desc, IID_PPV_ARGS(&pipeline));
  if (pipeline != nullptr) {
    pipeline->SetName(L"empty cs");
  }

  queue_desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
  result = device->CreateCommandQueue(&queue_desc, IID_PPV_ARGS(&queue));
  if (FAILED(result)) {
    std::fprintf(stderr, "CreateCommandQueue failed: 0x%08lx\n",
                 static_cast<unsigned long>(result));
    goto cleanup;
  }
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
    if (WaitForSingleObject(fence_event, 10000) != WAIT_OBJECT_0) {
      std::fprintf(stderr, "device stopped responding after the create\n");
      goto cleanup;
    }
  }

  if (create_result != E_INVALIDARG || pipeline != nullptr) {
    std::fprintf(stderr,
                 "empty CS: CreateComputePipelineState returned 0x%08lx, "
                 "pipeline %p\n",
                 static_cast<unsigned long>(create_result),
                 static_cast<void *>(pipeline));
    goto cleanup;
  }

  neptune_test::print_success(
      "empty_cs", "d3d12", timer.elapsed_ms(),
      neptune_test::checksum(&create_result, sizeof(create_result)));
  exit_code = 0;

cleanup:
  if (fence_event != nullptr) {
    CloseHandle(fence_event);
  }
  if (fence != nullptr) {
    fence->Release();
  }
  if (queue != nullptr) {
    queue->Release();
  }
  if (pipeline != nullptr) {
    pipeline->Release();
  }
  if (root_signature != nullptr) {
    root_signature->Release();
  }
  if (root_blob != nullptr) {
    root_blob->Release();
  }
  if (device != nullptr) {
    device->Release();
  }
  return exit_code;
}

#include "result.hpp"

#include <d3d12.h>
#include <dxgi1_4.h>

#include <cstdarg>
#include <cstdint>
#include <cstdio>

// DXGI fullscreen on a D3D12 flip-model swap chain: entering it must cover
// the output with the swap chain's window, and leaving it must restore the
// window. No ALLOW_MODE_SWITCH, so the desktop mode is left alone.

namespace {

constexpr UINT WINDOW_WIDTH = 640;
constexpr UINT WINDOW_HEIGHT = 400;

void pump_messages() {
  MSG msg;
  while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
    TranslateMessage(&msg);
    DispatchMessageW(&msg);
  }
}

// A GUI-subsystem process's stderr does not reach the Proton log, so
// failures also go through OutputDebugString like the result line.
void report(const char *format, ...) {
  char message[512];
  va_list args;
  va_start(args, format);
  std::vsnprintf(message, sizeof(message), format, args);
  va_end(args);
  OutputDebugStringA(message);
  std::fputs(message, stderr);
}

bool same_rect(const RECT &a, const RECT &b) {
  return a.left == b.left && a.top == b.top && a.right == b.right &&
         a.bottom == b.bottom;
}

void print_rect(const char *what, const RECT &r) {
  report("%s: %ld,%ld-%ld,%ld\n", what, r.left, r.top, r.right, r.bottom);
}

} // namespace

int main() {
  neptune_test::Timer timer;
  HWND hwnd = nullptr;
  ID3D12Device *device = nullptr;
  ID3D12CommandQueue *queue = nullptr;
  IDXGIFactory2 *factory = nullptr;
  IDXGISwapChain1 *swapchain = nullptr;
  IDXGIOutput *target = nullptr;
  D3D12_COMMAND_QUEUE_DESC queue_desc = {};
  DXGI_SWAP_CHAIN_DESC1 desc = {};
  WNDCLASSW window_class = {};
  MONITORINFO monitor = {};
  RECT windowed_rect = {};
  RECT fullscreen_rect = {};
  RECT restored_rect = {};
  BOOL fullscreen = FALSE;
  HRESULT result = S_OK;
  int exit_code = 1;

  window_class.lpfnWndProc = DefWindowProcW;
  window_class.hInstance = GetModuleHandleW(nullptr);
  window_class.lpszClassName = L"neptune_fullscreen_test";
  RegisterClassW(&window_class);
  hwnd = CreateWindowW(window_class.lpszClassName, L"neptune fullscreen test",
                       WS_OVERLAPPEDWINDOW, 64, 64, WINDOW_WIDTH, WINDOW_HEIGHT,
                       nullptr, nullptr, window_class.hInstance, nullptr);
  if (hwnd == nullptr) {
    report("CreateWindowW failed: %lu\n",
           static_cast<unsigned long>(GetLastError()));
    goto cleanup;
  }
  ShowWindow(hwnd, SW_SHOWNORMAL);
  pump_messages();
  GetWindowRect(hwnd, &windowed_rect);

  result =
      D3D12CreateDevice(nullptr, D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device));
  if (FAILED(result)) {
    report("D3D12CreateDevice failed: 0x%08lx\n",
           static_cast<unsigned long>(result));
    goto cleanup;
  }

  queue_desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
  result = device->CreateCommandQueue(&queue_desc, IID_PPV_ARGS(&queue));
  if (FAILED(result)) {
    report("CreateCommandQueue failed: 0x%08lx\n",
           static_cast<unsigned long>(result));
    goto cleanup;
  }

  result = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
  if (FAILED(result)) {
    report("CreateDXGIFactory1 failed: 0x%08lx\n",
           static_cast<unsigned long>(result));
    goto cleanup;
  }

  desc.Width = WINDOW_WIDTH;
  desc.Height = WINDOW_HEIGHT;
  desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
  desc.SampleDesc.Count = 1;
  desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
  desc.BufferCount = 2;
  desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
  result = factory->CreateSwapChainForHwnd(queue, hwnd, &desc, nullptr, nullptr,
                                           &swapchain);
  if (FAILED(result)) {
    report("CreateSwapChainForHwnd failed: 0x%08lx\n",
           static_cast<unsigned long>(result));
    goto cleanup;
  }

  result = swapchain->Present(0, 0);
  if (FAILED(result)) {
    report("windowed Present failed: 0x%08lx\n",
           static_cast<unsigned long>(result));
    goto cleanup;
  }

  result = swapchain->SetFullscreenState(TRUE, nullptr);
  if (FAILED(result)) {
    report("SetFullscreenState(TRUE) failed: 0x%08lx\n",
           static_cast<unsigned long>(result));
    goto cleanup;
  }
  pump_messages();

  result = swapchain->GetFullscreenState(&fullscreen, &target);
  if (FAILED(result) || !fullscreen || target == nullptr) {
    report("GetFullscreenState after enter: hr=0x%08lx fullscreen=%d "
           "target=%p\n",
           static_cast<unsigned long>(result), fullscreen,
           static_cast<void *>(target));
    goto cleanup;
  }

  monitor.cbSize = sizeof(monitor);
  GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY), &monitor);
  GetWindowRect(hwnd, &fullscreen_rect);
  if (!same_rect(fullscreen_rect, monitor.rcMonitor)) {
    report("fullscreen window does not cover the output\n");
    print_rect("window", fullscreen_rect);
    print_rect("monitor", monitor.rcMonitor);
    goto cleanup;
  }

  result = swapchain->Present(0, 0);
  if (FAILED(result)) {
    report("fullscreen Present failed: 0x%08lx\n",
           static_cast<unsigned long>(result));
    goto cleanup;
  }

  result = swapchain->SetFullscreenState(FALSE, nullptr);
  if (FAILED(result)) {
    report("SetFullscreenState(FALSE) failed: 0x%08lx\n",
           static_cast<unsigned long>(result));
    goto cleanup;
  }
  pump_messages();

  target->Release();
  target = nullptr;
  result = swapchain->GetFullscreenState(&fullscreen, &target);
  if (FAILED(result) || fullscreen || target != nullptr) {
    report("GetFullscreenState after leave: hr=0x%08lx fullscreen=%d "
           "target=%p\n",
           static_cast<unsigned long>(result), fullscreen,
           static_cast<void *>(target));
    goto cleanup;
  }

  GetWindowRect(hwnd, &restored_rect);
  if (!same_rect(restored_rect, windowed_rect)) {
    report("window rect not restored\n");
    print_rect("restored", restored_rect);
    print_rect("windowed", windowed_rect);
    goto cleanup;
  }

  {
    // The output size depends on the machine, so only the restored window
    // size goes into the checksum.
    const std::int32_t restored_size[] = {
        static_cast<std::int32_t>(restored_rect.right - restored_rect.left),
        static_cast<std::int32_t>(restored_rect.bottom - restored_rect.top)};
    neptune_test::print_success(
        "fullscreen", "d3d12", timer.elapsed_ms(),
        neptune_test::checksum(restored_size, sizeof(restored_size)));
  }
  exit_code = 0;

cleanup:
  if (target != nullptr) {
    target->Release();
  }
  if (swapchain != nullptr) {
    if (exit_code != 0) {
      swapchain->SetFullscreenState(FALSE, nullptr);
    }
    swapchain->Release();
  }
  if (factory != nullptr) {
    factory->Release();
  }
  if (queue != nullptr) {
    queue->Release();
  }
  if (device != nullptr) {
    device->Release();
  }
  if (hwnd != nullptr) {
    DestroyWindow(hwnd);
  }
  return exit_code;
}

// This must be included before many other Windows headers.
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include "device_kit_lib_plugin.h"

#include <shellapi.h>
#include <tlhelp32.h>
#include <uiautomationcoreapi.h>
#include <wincodec.h>
#include <winternl.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <cwctype>
#include <iomanip>
#include <iterator>
#include <limits>
#include <sstream>
#include <thread>

#pragma comment(lib, "gdi32.lib")
#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "shell32.lib")
#pragma comment(lib, "uiautomationcore.lib")
#pragma comment(lib, "user32.lib")
#pragma comment(lib, "windowscodecs.lib")

namespace device_kit_lib {
namespace {

using Microsoft::WRL::ComPtr;

constexpr size_t kMaximumUiNodes = 5000;
constexpr int kTargetWindowRetries = 40;
constexpr int kTargetWindowRetryDelayMs = 250;

char g_diagnostic_log_path[MAX_PATH]{};

void WriteCrashDiagnostic(EXCEPTION_POINTERS* exception_pointers) noexcept {
  if (exception_pointers == nullptr ||
      exception_pointers->ExceptionRecord == nullptr) {
    return;
  }
  const EXCEPTION_RECORD* record = exception_pointers->ExceptionRecord;
  if (record->ExceptionCode != EXCEPTION_ACCESS_VIOLATION &&
      record->ExceptionCode != EXCEPTION_ILLEGAL_INSTRUCTION &&
      record->ExceptionCode != EXCEPTION_STACK_OVERFLOW) {
    return;
  }

  const char* path = g_diagnostic_log_path[0] == '\0'
                         ? "device_kit_lib_windows.log"
                         : g_diagnostic_log_path;
  HANDLE file = CreateFileA(path, FILE_APPEND_DATA,
                            FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr,
                            OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) {
    return;
  }

  const HMODULE module = GetModuleHandleA("device_kit_lib_plugin.dll");
  char line[1024]{};
  const int length = std::snprintf(
      line, sizeof(line),
      "[CRASH] exception=0x%08lX; exception_address=%p; module_base=%p; "
      "thread=%lu; access_type=%llu; access_address=%p\r\n",
      static_cast<unsigned long>(record->ExceptionCode), record->ExceptionAddress,
      module, static_cast<unsigned long>(GetCurrentThreadId()),
      static_cast<unsigned long long>(record->NumberParameters >= 1
                                          ? record->ExceptionInformation[0]
                                          : 0),
      reinterpret_cast<void*>(record->NumberParameters >= 2
                                  ? record->ExceptionInformation[1]
                                  : 0));
  if (length > 0) {
    DWORD written = 0;
    WriteFile(file, line, static_cast<DWORD>(std::min<int>(length, sizeof(line) - 1)),
              &written, nullptr);
  }

  PVOID frames[32]{};
  const USHORT frame_count = CaptureStackBackTrace(0, std::size(frames), frames, nullptr);
  for (USHORT index = 0; index < frame_count; ++index) {
    const int frame_length = std::snprintf(line, sizeof(line),
                                           "[CRASH] frame[%u]=%p\r\n", index,
                                           frames[index]);
    if (frame_length > 0) {
      DWORD written = 0;
      WriteFile(file, line,
                static_cast<DWORD>(std::min<int>(frame_length, sizeof(line) - 1)),
                &written, nullptr);
    }
  }
  CloseHandle(file);
}

LONG CALLBACK VectoredExceptionLogger(EXCEPTION_POINTERS* exception_pointers) {
  WriteCrashDiagnostic(exception_pointers);
  return EXCEPTION_CONTINUE_SEARCH;
}

std::string WideToUtf8(const std::wstring& value) {
  if (value.empty()) {
    return {};
  }
  const int size = WideCharToMultiByte(
      CP_UTF8, WC_ERR_INVALID_CHARS, value.data(), static_cast<int>(value.size()),
      nullptr, 0, nullptr, nullptr);
  if (size <= 0) {
    return {};
  }
  std::string result(static_cast<size_t>(size), '\0');
  WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                      static_cast<int>(value.size()), result.data(), size, nullptr,
                      nullptr);
  return result;
}

std::wstring Utf8ToWide(const std::string& value) {
  if (value.empty()) {
    return {};
  }
  const int size = MultiByteToWideChar(
      CP_UTF8, MB_ERR_INVALID_CHARS, value.data(), static_cast<int>(value.size()),
      nullptr, 0);
  if (size <= 0) {
    return {};
  }
  std::wstring result(static_cast<size_t>(size), L'\0');
  MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, value.data(),
                      static_cast<int>(value.size()), result.data(), size);
  return result;
}

std::string BstrToUtf8(BSTR value) {
  if (value == nullptr) {
    return {};
  }
  return WideToUtf8(std::wstring(value, SysStringLen(value)));
}

std::string Win32ErrorMessage(DWORD error) {
  if (error == ERROR_SUCCESS) {
    return {};
  }
  LPWSTR buffer = nullptr;
  const DWORD flags = FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM |
                      FORMAT_MESSAGE_IGNORE_INSERTS;
  const DWORD length = FormatMessageW(flags, nullptr, error, 0,
                                      reinterpret_cast<LPWSTR>(&buffer), 0, nullptr);
  std::wstring message = length == 0 ? L"unknown Win32 error" : std::wstring(buffer, length);
  if (buffer != nullptr) {
    LocalFree(buffer);
  }
  while (!message.empty() && (message.back() == L'\r' || message.back() == L'\n')) {
    message.pop_back();
  }
  return WideToUtf8(message);
}

std::string HResultString(HRESULT hr) {
  if (SUCCEEDED(hr)) {
    return {};
  }
  std::ostringstream stream;
  stream << "0x" << std::uppercase << std::hex
         << static_cast<unsigned long>(hr);
  return stream.str();
}

std::wstring Lowercase(std::wstring value) {
  std::transform(value.begin(), value.end(), value.begin(),
                 [](wchar_t character) { return std::towlower(character); });
  return value;
}

std::wstring FileNameOf(const std::wstring& path) {
  const size_t slash = path.find_last_of(L"\\/");
  return slash == std::wstring::npos ? path : path.substr(slash + 1);
}

std::wstring EnsureExeSuffix(std::wstring value) {
  const std::wstring file_name = FileNameOf(value);
  if (file_name.find(L'.') == std::wstring::npos) {
    value += L".exe";
  }
  return value;
}

std::optional<std::string> OptionalString(const std::string& value) {
  return value.empty() ? std::nullopt : std::optional<std::string>(value);
}

std::string RoleForControlType(int control_type) {
  switch (control_type) {
    case UIA_ButtonControlTypeId:
      return "button";
    case UIA_CheckBoxControlTypeId:
      return "checkbox";
    case UIA_ComboBoxControlTypeId:
      return "combobox";
    case UIA_EditControlTypeId:
      return "textField";
    case UIA_HyperlinkControlTypeId:
      return "link";
    case UIA_ImageControlTypeId:
      return "image";
    case UIA_ListControlTypeId:
      return "list";
    case UIA_ListItemControlTypeId:
      return "listItem";
    case UIA_MenuControlTypeId:
      return "menu";
    case UIA_MenuItemControlTypeId:
      return "menuItem";
    case UIA_ProgressBarControlTypeId:
      return "progressBar";
    case UIA_RadioButtonControlTypeId:
      return "radio";
    case UIA_ScrollBarControlTypeId:
      return "scrollView";
    case UIA_SliderControlTypeId:
      return "slider";
    case UIA_TabControlTypeId:
      return "list";
    case UIA_TextControlTypeId:
      return "text";
    case UIA_TreeControlTypeId:
      return "list";
    case UIA_TreeItemControlTypeId:
      return "listItem";
    case UIA_WindowControlTypeId:
      return "window";
    default:
      return "unknown";
  }
}

bool IsProcessNameMatch(const std::wstring& process_name,
                        const std::wstring& target) {
  const std::wstring process = Lowercase(FileNameOf(process_name));
  std::wstring target_name = Lowercase(FileNameOf(target));
  if (target_name.find(L'.') == std::wstring::npos) {
    target_name += L".exe";
  }
  return process == target_name;
}

std::string FormatOperationFailure(const std::string& detail, HRESULT hr,
                                   DWORD win32_error) {
  std::ostringstream message;
  message << detail;
  if (FAILED(hr)) {
    message << "; hresult=" << HResultString(hr);
  }
  if (win32_error != ERROR_SUCCESS) {
    message << "; win32=" << win32_error;
    const std::string system_message = Win32ErrorMessage(win32_error);
    if (!system_message.empty()) {
      message << " (" << system_message << ")";
    }
  }
  return message.str();
}

FlutterError MakeFlutterError(const std::string& message) {
  return FlutterError("windows-error", message);
}

template <typename T>
ErrorOr<T> Failed(const std::string& message) {
  return ErrorOr<T>(MakeFlutterError(message));
}

struct ScopedHdc {
  HDC screen = nullptr;
  HDC value = nullptr;
  HGDIOBJ previous = nullptr;
  ~ScopedHdc() {
    if (value != nullptr) {
      if (previous != nullptr) {
        SelectObject(value, previous);
      }
      DeleteDC(value);
    }
    if (screen != nullptr) {
      ReleaseDC(nullptr, screen);
    }
  }
};

struct ScopedBitmap {
  HBITMAP value = nullptr;
  HDC dc = nullptr;
  HGDIOBJ previous = nullptr;
  ~ScopedBitmap() {
    if (value != nullptr) {
      if (dc != nullptr && previous != nullptr) {
        SelectObject(dc, previous);
      }
      DeleteObject(value);
    }
  }
};

ErrorOr<std::vector<uint8_t>> CaptureDesktopPng() {
  ScopedHdc dc;
  dc.screen = GetDC(nullptr);
  if (dc.screen == nullptr) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("GetDC failed", S_OK, GetLastError()));
  }

  const int left = GetSystemMetrics(SM_XVIRTUALSCREEN);
  const int top = GetSystemMetrics(SM_YVIRTUALSCREEN);
  const int width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
  const int height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
  if (width <= 0 || height <= 0) {
    return Failed<std::vector<uint8_t>>("The Windows virtual desktop has no size.");
  }

  dc.value = CreateCompatibleDC(dc.screen);
  if (dc.value == nullptr) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("CreateCompatibleDC failed", S_OK, GetLastError()));
  }

  ScopedBitmap bitmap;
  bitmap.value = CreateCompatibleBitmap(dc.screen, width, height);
  if (bitmap.value == nullptr) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("CreateCompatibleBitmap failed", S_OK, GetLastError()));
  }
  dc.previous = SelectObject(dc.value, bitmap.value);
  if (dc.previous == nullptr || dc.previous == HGDI_ERROR) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("SelectObject failed", S_OK, GetLastError()));
  }
  bitmap.dc = dc.value;
  bitmap.previous = dc.previous;
  dc.previous = nullptr;

  if (!BitBlt(dc.value, 0, 0, width, height, dc.screen, left, top,
              SRCCOPY | CAPTUREBLT)) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("BitBlt failed", S_OK, GetLastError()));
  }

  ComPtr<IWICImagingFactory> factory;
  HRESULT hr = CoCreateInstance(CLSID_WICImagingFactory2, nullptr,
                                CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory));
  if (FAILED(hr)) {
    hr = CoCreateInstance(CLSID_WICImagingFactory, nullptr,
                          CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory));
  }
  if (FAILED(hr)) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("WIC imaging factory creation failed", hr, GetLastError()));
  }

  ComPtr<IWICBitmap> wic_bitmap;
  hr = factory->CreateBitmapFromHBITMAP(bitmap.value, nullptr, WICBitmapIgnoreAlpha,
                                        &wic_bitmap);
  if (FAILED(hr)) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("WIC bitmap creation failed", hr, GetLastError()));
  }

  ComPtr<IStream> stream;
  hr = CreateStreamOnHGlobal(nullptr, TRUE, &stream);
  if (FAILED(hr)) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("CreateStreamOnHGlobal failed", hr, GetLastError()));
  }

  ComPtr<IWICBitmapEncoder> encoder;
  hr = factory->CreateEncoder(GUID_ContainerFormatPng, nullptr, &encoder);
  if (FAILED(hr)) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("WIC PNG encoder creation failed", hr, GetLastError()));
  }
  hr = encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache);
  if (FAILED(hr)) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("WIC encoder initialization failed", hr, GetLastError()));
  }

  ComPtr<IWICBitmapFrameEncode> frame;
  ComPtr<IPropertyBag2> properties;
  hr = encoder->CreateNewFrame(&frame, &properties);
  if (FAILED(hr)) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("WIC frame creation failed", hr, GetLastError()));
  }
  hr = frame->Initialize(properties.Get());
  if (SUCCEEDED(hr)) {
    hr = frame->SetSize(static_cast<UINT>(width), static_cast<UINT>(height));
  }
  if (SUCCEEDED(hr)) {
    WICPixelFormatGUID format = GUID_WICPixelFormat32bppBGRA;
    hr = frame->SetPixelFormat(&format);
  }
  if (SUCCEEDED(hr)) {
    hr = frame->WriteSource(wic_bitmap.Get(), nullptr);
  }
  if (SUCCEEDED(hr)) {
    hr = frame->Commit();
  }
  if (SUCCEEDED(hr)) {
    hr = encoder->Commit();
  }
  if (FAILED(hr)) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("WIC PNG encoding failed", hr, GetLastError()));
  }

  STATSTG statistics{};
  hr = stream->Stat(&statistics, STATFLAG_NONAME);
  if (FAILED(hr) || statistics.cbSize.QuadPart <= 0 ||
      statistics.cbSize.QuadPart > (std::numeric_limits<size_t>::max)()) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("Unable to read encoded PNG size", hr, GetLastError()));
  }
  const size_t size = static_cast<size_t>(statistics.cbSize.QuadPart);
  LARGE_INTEGER origin{};
  hr = stream->Seek(origin, STREAM_SEEK_SET, nullptr);
  if (FAILED(hr)) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("Unable to rewind encoded PNG stream", hr, GetLastError()));
  }
  HGLOBAL global = nullptr;
  hr = GetHGlobalFromStream(stream.Get(), &global);
  if (FAILED(hr) || global == nullptr) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("Unable to access encoded PNG memory", hr, GetLastError()));
  }
  void* bytes = GlobalLock(global);
  if (bytes == nullptr) {
    return Failed<std::vector<uint8_t>>(
        FormatOperationFailure("GlobalLock for encoded PNG failed", S_OK, GetLastError()));
  }
  std::vector<uint8_t> result(static_cast<uint8_t*>(bytes),
                              static_cast<uint8_t*>(bytes) + size);
  GlobalUnlock(global);
  return result;
}

}  // namespace

// static
void DeviceKitLibPlugin::RegisterWithRegistrar(
    flutter::PluginRegistrarWindows* registrar) {
  auto plugin = std::make_unique<DeviceKitLibPlugin>();
  DeviceKitHostApi::SetUp(registrar->messenger(), plugin.get());
  registrar->AddPlugin(std::move(plugin));
}

DeviceKitLibPlugin::DeviceKitLibPlugin() = default;

DeviceKitLibPlugin::~DeviceKitLibPlugin() {
  Dispose();
}

bool DeviceKitLibPlugin::EnsureComInitialized(const char* operation) {
  const DWORD current_thread = GetCurrentThreadId();
  if (com_initialized_) {
    if (com_thread_id_ != current_thread) {
      Log("ERROR", operation,
          "COM was initialized on another thread; UI Automation calls must stay on the "
          "plugin platform thread",
          RPC_E_WRONG_THREAD, ERROR_SUCCESS, true);
      return false;
    }
    return true;
  }

  com_init_result_ = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
  if (FAILED(com_init_result_) && com_init_result_ != RPC_E_CHANGED_MODE) {
    Log("ERROR", operation, "CoInitializeEx failed", com_init_result_,
        GetLastError(), true);
    return false;
  }
  com_initialized_ = true;
  com_thread_id_ = current_thread;
  if (com_init_result_ == RPC_E_CHANGED_MODE) {
    Log("WARN", operation,
        "COM is already initialized with a different apartment model; continuing "
        "without calling CoUninitialize",
        com_init_result_, ERROR_SUCCESS, true);
  }
  return true;
}

bool DeviceKitLibPlugin::EnsureAutomation(const char* operation) {
  if (!EnsureComInitialized(operation)) {
    return false;
  }
  if (automation_ != nullptr && raw_view_walker_ != nullptr) {
    return true;
  }

  HRESULT hr = CoCreateInstance(CLSID_CUIAutomation8, nullptr,
                                CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&automation_));
  if (FAILED(hr)) {
    hr = CoCreateInstance(CLSID_CUIAutomation, nullptr, CLSCTX_INPROC_SERVER,
                          IID_PPV_ARGS(&automation_));
  }
  if (FAILED(hr)) {
    Log("ERROR", operation, "Unable to create the Windows UI Automation client", hr,
        GetLastError(), true);
    return false;
  }

  hr = automation_->get_RawViewWalker(&raw_view_walker_);
  if (FAILED(hr) || raw_view_walker_ == nullptr) {
    Log("ERROR", operation, "Unable to create the UI Automation raw-view walker",
        hr, GetLastError(), true);
    automation_.Reset();
    return false;
  }
  Log("INFO", operation,
      "Windows UI Automation client initialized; traversal=raw-view", hr);
  return true;
}

std::optional<uint32_t> DeviceKitLibPlugin::FindProcessId(
    const std::wstring& target) const {
  const std::wstring target_name = EnsureExeSuffix(FileNameOf(target));
  HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
  if (snapshot == INVALID_HANDLE_VALUE) {
    return std::nullopt;
  }

  PROCESSENTRY32W process_entry{};
  process_entry.dwSize = sizeof(process_entry);
  std::optional<uint32_t> result;
  if (Process32FirstW(snapshot, &process_entry) != FALSE) {
    do {
      if (IsProcessNameMatch(process_entry.szExeFile, target_name)) {
        result = process_entry.th32ProcessID;
        break;
      }
    } while (Process32NextW(snapshot, &process_entry) != FALSE);
  }
  CloseHandle(snapshot);
  return result;
}

std::optional<uint32_t> DeviceKitLibPlugin::StartTarget(
    const std::wstring& target) {
  SHELLEXECUTEINFOW execute_info{};
  execute_info.cbSize = sizeof(execute_info);
  execute_info.fMask = SEE_MASK_NOCLOSEPROCESS;
  execute_info.lpVerb = L"open";
  execute_info.lpFile = target.c_str();
  execute_info.nShow = SW_SHOWNORMAL;
  if (ShellExecuteExW(&execute_info) == FALSE) {
    Log("ERROR", "launchApp", "ShellExecuteExW failed for target " + WideToUtf8(target),
        S_OK, GetLastError(), true);
    return std::nullopt;
  }

  std::optional<uint32_t> process_id;
  if (execute_info.hProcess != nullptr) {
    process_id = GetProcessId(execute_info.hProcess);
    if (process_id.has_value()) {
      const DWORD wait_result = WaitForInputIdle(execute_info.hProcess, 5000);
      Log("INFO", "launchApp",
          "Target process started with pid " + std::to_string(*process_id) +
              "; WaitForInputIdle=" + std::to_string(wait_result));
    }
    CloseHandle(execute_info.hProcess);
  }
  return process_id.has_value() ? process_id : FindProcessId(target);
}

void DeviceKitLibPlugin::ResetTarget() {
  target_root_.Reset();
  target_process_id_ = 0;
  target_name_.clear();
  elements_by_node_id_.clear();
  pending_nodes_.clear();
}

bool DeviceKitLibPlugin::ActivateTargetWindow(const char* operation) {
  if (target_root_ == nullptr) {
    return false;
  }
  UIA_HWND window_handle = 0;
  HRESULT hr = target_root_->get_CurrentNativeWindowHandle(&window_handle);
  if (FAILED(hr) || window_handle == 0) {
    Log("WARN", operation, "Target UIA root has no native window handle", hr,
        GetLastError(), true);
    return false;
  }
  HWND window = reinterpret_cast<HWND>(window_handle);
  ShowWindow(window, SW_RESTORE);
  SetForegroundWindow(window);
  hr = target_root_->SetFocus();
  if (FAILED(hr)) {
    Log("WARN", operation, "UIA SetFocus failed after activating target window", hr,
        GetLastError(), true);
  }
  return true;
}

bool DeviceKitLibPlugin::ResolveTargetRoot(const char* operation, bool retry) {
  if (!EnsureAutomation(operation)) {
    return false;
  }
  if (target_root_ != nullptr) {
    return ActivateTargetWindow(operation);
  }

  if (target_process_id_ == 0) {
    HWND foreground = GetForegroundWindow();
    if (foreground != nullptr) {
      DWORD process_id = 0;
      GetWindowThreadProcessId(foreground, &process_id);
      target_process_id_ = process_id;
      Log("INFO", operation,
          "No launch pid was returned; using foreground process " +
              std::to_string(process_id));
    }
  }
  if (target_process_id_ == 0) {
    Log("ERROR", operation, "Target process id is not available", S_OK, ERROR_NOT_FOUND,
        true);
    return false;
  }

  VARIANT process_value;
  VariantInit(&process_value);
  process_value.vt = VT_I4;
  process_value.lVal = static_cast<LONG>(target_process_id_);
  ComPtr<IUIAutomationCondition> process_condition;
  HRESULT hr = automation_->CreatePropertyCondition(
      UIA_ProcessIdPropertyId, process_value, &process_condition);
  VariantClear(&process_value);
  if (FAILED(hr)) {
    Log("ERROR", operation, "Unable to create UIA process condition", hr,
        GetLastError(), true);
    return false;
  }

  const int attempts = retry ? kTargetWindowRetries : 1;
  for (int attempt = 0; attempt < attempts; ++attempt) {
    ComPtr<IUIAutomationElement> desktop;
    hr = automation_->GetRootElement(&desktop);
    if (SUCCEEDED(hr) && desktop != nullptr) {
      hr = desktop->FindFirst(TreeScope_Subtree, process_condition.Get(),
                              &target_root_);
      if (SUCCEEDED(hr) && target_root_ != nullptr) {
        Log("INFO", operation,
            "Resolved target UIA root for pid " + std::to_string(target_process_id_) +
                " after attempt " + std::to_string(attempt + 1), hr);
        ActivateTargetWindow(operation);
        return true;
      }
    }
    target_root_.Reset();
    if (attempt + 1 < attempts) {
      std::this_thread::sleep_for(
          std::chrono::milliseconds(kTargetWindowRetryDelayMs));
    }
  }

  Log("ERROR", operation,
      "UIA could not find a window for target pid " +
          std::to_string(target_process_id_) + ". Target may be elevated, running "
          "in another desktop/session, or still starting.",
      hr, GetLastError(), true);
  return false;
}

bool DeviceKitLibPlugin::ReadElement(const AutomationElement& element,
                                     PendingNode* node) {
  if (element == nullptr || node == nullptr) {
    return false;
  }

  const std::string trace_prefix = "node=" + node->node_id + "; index=" +
                                   std::to_string(pending_nodes_.size()) + "; ";
  Log("TRACE", "dumpUi", trace_prefix + "ReadElement begin", S_OK,
      ERROR_SUCCESS, true);

  auto read_bstr = [&](const char* property,
                       auto getter) -> std::optional<std::string> {
    Log("TRACE", "dumpUi", trace_prefix + "before " + property, S_OK,
        ERROR_SUCCESS, true);
    BSTR value = nullptr;
    const HRESULT hr = getter(&value);
    const std::string trace_value =
        value == nullptr ? "<null>" : BstrToUtf8(value);
    Log("TRACE", "dumpUi",
        trace_prefix + "after " + property + "; value=" + trace_value,
        hr, FAILED(hr) ? GetLastError() : ERROR_SUCCESS, true);
    if (FAILED(hr)) {
      if (hr != UIA_E_ELEMENTNOTAVAILABLE) {
        Log("DEBUG", "dumpUi", "UIA string property read failed", hr,
            GetLastError());
      }
      return std::nullopt;
    }
    const std::string result = BstrToUtf8(value);
    SysFreeString(value);
    return OptionalString(result);
  };

  int control_type = UIA_CustomControlTypeId;
  Log("TRACE", "dumpUi", trace_prefix + "before CurrentControlType", S_OK,
      ERROR_SUCCESS, true);
  const HRESULT control_type_hr =
      element->get_CurrentControlType(&control_type);
  Log("TRACE", "dumpUi", trace_prefix + "after CurrentControlType; type=" +
                            std::to_string(control_type),
      control_type_hr,
      FAILED(control_type_hr) ? GetLastError() : ERROR_SUCCESS, true);
  if (FAILED(control_type_hr)) {
    control_type = UIA_CustomControlTypeId;
  }
  node->role = RoleForControlType(control_type);
  node->automation_id = read_bstr("CurrentAutomationId",
      [&](BSTR* value) { return element->get_CurrentAutomationId(value); });
  node->text = read_bstr("CurrentName",
      [&](BSTR* value) { return element->get_CurrentName(value); });
  node->label = read_bstr("CurrentHelpText",
      [&](BSTR* value) { return element->get_CurrentHelpText(value); });

  RECT rectangle{};
  Log("TRACE", "dumpUi", trace_prefix + "before CurrentBoundingRectangle", S_OK,
      ERROR_SUCCESS, true);
  const HRESULT bounds_hr = element->get_CurrentBoundingRectangle(&rectangle);
  Log("TRACE", "dumpUi", trace_prefix + "after CurrentBoundingRectangle",
      bounds_hr, FAILED(bounds_hr) ? GetLastError() : ERROR_SUCCESS, true);
  if (FAILED(bounds_hr)) {
    rectangle = RECT{};
  }
  node->bounds = RectData(
      static_cast<double>(rectangle.left), static_cast<double>(rectangle.top),
      static_cast<double>(std::max<LONG>(0, rectangle.right - rectangle.left)),
      static_cast<double>(std::max<LONG>(0, rectangle.bottom - rectangle.top)));
  BOOL enabled = TRUE;
  BOOL focused = FALSE;
  Log("TRACE", "dumpUi", trace_prefix + "before CurrentIsEnabled", S_OK,
      ERROR_SUCCESS, true);
  const HRESULT enabled_hr = element->get_CurrentIsEnabled(&enabled);
  Log("TRACE", "dumpUi", trace_prefix + "after CurrentIsEnabled", enabled_hr,
      FAILED(enabled_hr) ? GetLastError() : ERROR_SUCCESS, true);
  Log("TRACE", "dumpUi", trace_prefix + "before CurrentHasKeyboardFocus", S_OK,
      ERROR_SUCCESS, true);
  const HRESULT focused_hr = element->get_CurrentHasKeyboardFocus(&focused);
  Log("TRACE", "dumpUi", trace_prefix + "after CurrentHasKeyboardFocus", focused_hr,
      FAILED(focused_hr) ? GetLastError() : ERROR_SUCCESS, true);
  node->enabled = enabled != FALSE;
  node->focused = focused != FALSE;

  // Querying every pattern through Flutter's Windows UIA provider is not safe:
  // some provider implementations return a stale pattern proxy and crash the
  // client instead of returning UIA_E_ELEMENTNOTAVAILABLE. Infer the common
  // capabilities from the control type during a tree dump and resolve the
  // actual pattern only when an action is requested.
  node->clickable = control_type == UIA_ButtonControlTypeId ||
                    control_type == UIA_HyperlinkControlTypeId ||
                    control_type == UIA_MenuItemControlTypeId ||
                    control_type == UIA_RadioButtonControlTypeId ||
                    control_type == UIA_CheckBoxControlTypeId;
  node->editable = control_type == UIA_EditControlTypeId;
  node->scrollable = control_type == UIA_ListControlTypeId ||
                     control_type == UIA_TreeControlTypeId ||
                     control_type == UIA_ScrollBarControlTypeId;
  if (node->label == std::nullopt && node->text.has_value()) {
    node->label = node->text;
  }
  Log("TRACE", "dumpUi", trace_prefix + "ReadElement complete", S_OK,
      ERROR_SUCCESS, true);
  return true;
}

std::optional<std::string> DeviceKitLibPlugin::AppendElementTree(
    const AutomationElement& element,
    const std::optional<std::string>& parent_node_id,
    ComPtr<IUIAutomationTreeWalker> walker,
    const char* operation) {
  if (element == nullptr || walker == nullptr ||
      pending_nodes_.size() >= kMaximumUiNodes) {
    Log("WARN", operation,
        "UIA tree traversal reached an invalid element or the node safety limit");
    return std::nullopt;
  }

  PendingNode node;
  node.node_id = "windows-" + std::to_string(generation_) + "-" +
                 std::to_string(pending_nodes_.size());
  Log("TRACE", operation,
      "before ReadElement; node=" + node.node_id + "; parent=" +
          (parent_node_id.has_value() ? *parent_node_id : "<root>"),
      S_OK, ERROR_SUCCESS, true);
  node.parent_node_id = parent_node_id;
  node.element = element;
  if (!ReadElement(element, &node)) {
    Log("WARN", operation, "ReadElement returned false; node=" + node.node_id,
        S_OK, ERROR_SUCCESS, true);
    return std::nullopt;
  }
  const std::string node_id = node.node_id;
  pending_nodes_.push_back(std::move(node));
  elements_by_node_id_[node_id] = element;

  ComPtr<IUIAutomationElement> child;
  Log("TRACE", operation, "before GetFirstChildElement; node=" + node_id,
      S_OK, ERROR_SUCCESS, true);
  HRESULT hr = walker->GetFirstChildElement(element.Get(), &child);
  Log("TRACE", operation,
      "after GetFirstChildElement; node=" + node_id + "; child=" +
          (child == nullptr ? "null" : "present"),
      hr, FAILED(hr) ? GetLastError() : ERROR_SUCCESS, true);
  while (SUCCEEDED(hr) && child != nullptr) {
    Log("TRACE", operation,
        "before child recursion; parent=" + node_id + "; node_count=" +
            std::to_string(pending_nodes_.size()),
        S_OK, ERROR_SUCCESS, true);
    const std::optional<std::string> child_id =
        AppendElementTree(child, node_id, walker, operation);
    if (child_id.has_value()) {
      auto parent = std::find_if(
          pending_nodes_.begin(), pending_nodes_.end(),
          [&](const PendingNode& candidate) { return candidate.node_id == node_id; });
      if (parent != pending_nodes_.end()) {
        parent->child_node_ids.push_back(*child_id);
      }
    }
    if (pending_nodes_.size() >= kMaximumUiNodes) {
      Log("WARN", operation, "UIA tree traversal reached the node safety limit");
      break;
    }
    ComPtr<IUIAutomationElement> next_sibling;
    Log("TRACE", operation, "before GetNextSiblingElement; node=" + node_id,
        S_OK, ERROR_SUCCESS, true);
    hr = walker->GetNextSiblingElement(child.Get(), &next_sibling);
    Log("TRACE", operation,
        "after GetNextSiblingElement; node=" + node_id + "; sibling=" +
            (next_sibling == nullptr ? "null" : "present"),
        hr, FAILED(hr) ? GetLastError() : ERROR_SUCCESS, true);
    if (next_sibling.Get() == child.Get()) {
      Log("WARN", operation, "UIA provider returned the same sibling element");
      break;
    }
    child = next_sibling;
  }
  if (FAILED(hr) && hr != UIA_E_ELEMENTNOTAVAILABLE) {
    Log("DEBUG", operation, "UIA child traversal ended with an error", hr,
        GetLastError());
  }
  return node_id;
}

std::optional<FlutterError> DeviceKitLibPlugin::Initialize(
    const DriverConfig& config) {
  std::lock_guard<std::mutex> lock(mutex_);
  session_id_ = config.session_id();
  // Keep native diagnostic evidence even when the Dart configuration did not
  // explicitly enable logs. This is needed to identify an in-process access
  // violation, where Dart receives no error callback.
  logging_enabled_ = true;
  wchar_t environment_value[8]{};
  if (GetEnvironmentVariableW(L"DEVICE_KIT_LIB_WINDOWS_LOG", environment_value,
                              static_cast<DWORD>(std::size(environment_value))) > 0 &&
      std::wstring(environment_value) != L"0") {
    logging_enabled_ = true;
  }
  char temp_path[MAX_PATH]{};
  const DWORD temp_capacity = static_cast<DWORD>(sizeof(temp_path));
  const DWORD temp_length = GetTempPathA(temp_capacity, temp_path);
  log_path_ = temp_length == 0 || temp_length >= temp_capacity
                  ? "device_kit_lib_windows.log"
                  : std::string(temp_path, temp_length) + "device_kit_lib_windows.log";
  strncpy_s(g_diagnostic_log_path, log_path_.c_str(), _TRUNCATE);
  Log("INFO", "initialize", "log_file=" + log_path_, S_OK, ERROR_SUCCESS, true);
  if (crash_log_handler_ == nullptr) {
    crash_log_handler_ = AddVectoredExceptionHandler(1, VectoredExceptionLogger);
    Log(crash_log_handler_ == nullptr ? "ERROR" : "INFO", "initialize",
        crash_log_handler_ == nullptr ? "Unable to register crash diagnostics"
                                      : "Crash diagnostics registered",
        crash_log_handler_ == nullptr ? E_FAIL : S_OK,
        crash_log_handler_ == nullptr ? GetLastError() : ERROR_SUCCESS, true);
  }

  ResetTarget();
  generation_ = 0;
  initialized_ = false;
  Log("INFO", "initialize",
      "session=" + session_id_ + "; logs=" + (logging_enabled_ ? "on" : "off"));
  if (!EnsureAutomation("initialize")) {
    return MakeFlutterError(OperationFailure(
        "initialize", "Windows UI Automation is unavailable", E_NOINTERFACE));
  }
  initialized_ = true;
  Log("INFO", "initialize", "Windows Device Kit backend is ready");
  return std::nullopt;
}

std::optional<FlutterError> DeviceKitLibPlugin::Dispose() {
  std::lock_guard<std::mutex> lock(mutex_);
  Log("INFO", "dispose", "Releasing Windows UI Automation session");
  if (crash_log_handler_ != nullptr) {
    RemoveVectoredExceptionHandler(crash_log_handler_);
    crash_log_handler_ = nullptr;
  }
  ResetTarget();
  raw_view_walker_.Reset();
  automation_.Reset();
  initialized_ = false;
  if (com_initialized_ && com_init_result_ != RPC_E_CHANGED_MODE &&
      com_thread_id_ == GetCurrentThreadId()) {
    CoUninitialize();
  }
  com_initialized_ = false;
  com_init_result_ = E_NOTIMPL;
  com_thread_id_ = 0;
  return std::nullopt;
}

ErrorOr<DeviceInfo> DeviceKitLibPlugin::GetDeviceInfo() {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!initialized_) {
    return Failed<DeviceInfo>(OperationFailure(
        "getDeviceInfo", "The Windows driver is not initialized"));
  }

  OSVERSIONINFOEXW version{};
  version.dwOSVersionInfoSize = sizeof(version);
  using RtlGetVersionFunction = LONG(WINAPI*)(PRTL_OSVERSIONINFOEXW);
  const HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
  const auto get_version = ntdll == nullptr
                               ? nullptr
                               : reinterpret_cast<RtlGetVersionFunction>(
                                     GetProcAddress(ntdll, "RtlGetVersion"));
  std::string os_version = "unknown";
  if (get_version != nullptr && get_version(&version) == 0) {
    os_version = std::to_string(version.dwMajorVersion) + "." +
                 std::to_string(version.dwMinorVersion) + "." +
                 std::to_string(version.dwBuildNumber);
  }
  wchar_t computer_name[MAX_COMPUTERNAME_LENGTH + 1]{};
  DWORD computer_name_length = static_cast<DWORD>(std::size(computer_name));
  std::string device_name;
  if (GetComputerNameW(computer_name, &computer_name_length) != FALSE) {
    device_name = WideToUtf8(computer_name);
  }
  Log("INFO", "getDeviceInfo", "Windows version=" + os_version);
  return DeviceInfo("Windows", &os_version, nullptr,
                    device_name.empty() ? nullptr : &device_name, true);
}

ErrorOr<ActionResult> DeviceKitLibPlugin::OpenAccessibilitySettings() {
  std::lock_guard<std::mutex> lock(mutex_);
  const HINSTANCE result = ShellExecuteW(nullptr, L"open", L"ms-settings:easeofaccess",
                                         nullptr, nullptr, SW_SHOWNORMAL);
  if (reinterpret_cast<INT_PTR>(result) <= 32) {
    return Failed<ActionResult>(OperationFailure(
        "openAccessibilitySettings", "Unable to open Windows Ease of Access settings",
        S_OK, GetLastError()));
  }
  Log("INFO", "openAccessibilitySettings", "Windows Ease of Access settings opened");
  return Success(false);
}

ErrorOr<ActionResult> DeviceKitLibPlugin::LaunchApp(
    const std::string& package_name) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!initialized_) {
    return Failed<ActionResult>(OperationFailure(
        "launchApp", "The Windows driver is not initialized"));
  }
  const std::wstring target = Utf8ToWide(package_name);
  if (target.empty()) {
    return Failed<ActionResult>(OperationFailure(
        "launchApp", "The Windows target path or executable name is empty"));
  }
  ResetTarget();
  target_name_ = target;
  Log("INFO", "launchApp", "target=" + WideToUtf8(target));

  std::optional<uint32_t> process_id = FindProcessId(target);
  if (process_id.has_value()) {
    target_process_id_ = *process_id;
    Log("INFO", "launchApp", "Using existing target process pid " +
                                  std::to_string(target_process_id_));
  } else {
    process_id = StartTarget(target);
    if (process_id.has_value()) {
      target_process_id_ = *process_id;
    }
  }
  if (target_process_id_ == 0) {
    return Failed<ActionResult>(OperationFailure(
        "launchApp", "Target was not launched and no matching process was found",
        S_OK, ERROR_FILE_NOT_FOUND));
  }
  return Success(false);
}

ErrorOr<UiSnapshot> DeviceKitLibPlugin::DumpUi() {
  std::lock_guard<std::mutex> lock(mutex_);
  Log("TRACE", "dumpUi", "DumpUi entered", S_OK, ERROR_SUCCESS, true);
  if (!initialized_) {
    return Failed<UiSnapshot>(OperationFailure(
        "dumpUi", "The Windows driver is not initialized"));
  }
  if (!ResolveTargetRoot("dumpUi", true)) {
    return Failed<UiSnapshot>(OperationFailure(
        "dumpUi", "Unable to resolve the target window through UI Automation"));
  }
  Log("TRACE", "dumpUi", "Target UIA root resolved", S_OK, ERROR_SUCCESS,
      true);

  ++generation_;
  pending_nodes_.clear();
  elements_by_node_id_.clear();
  Log("TRACE", "dumpUi", "before AppendElementTree", S_OK, ERROR_SUCCESS,
      true);
  const std::optional<std::string> root_id =
      AppendElementTree(target_root_, std::nullopt, raw_view_walker_, "dumpUi");
  Log("TRACE", "dumpUi",
      "after AppendElementTree; root=" +
          (root_id.has_value() ? *root_id : "<none>") + "; node_count=" +
          std::to_string(pending_nodes_.size()),
      S_OK, ERROR_SUCCESS, true);
  if (!root_id.has_value() || pending_nodes_.empty()) {
    return Failed<UiSnapshot>(OperationFailure(
        "dumpUi", "UI Automation returned an empty target tree"));
  }

  flutter::EncodableList nodes;
  nodes.reserve(pending_nodes_.size());
  Log("TRACE", "dumpUi", "before Flutter node serialization", S_OK,
      ERROR_SUCCESS, true);
  for (const PendingNode& pending : pending_nodes_) {
    flutter::EncodableList child_ids;
    child_ids.reserve(pending.child_node_ids.size());
    for (const std::string& child_id : pending.child_node_ids) {
      child_ids.emplace_back(child_id);
    }
    const std::string* parent = pending.parent_node_id.has_value()
                                    ? &pending.parent_node_id.value()
                                    : nullptr;
    const std::string* automation_id = pending.automation_id.has_value()
                                           ? &pending.automation_id.value()
                                           : nullptr;
    const std::string* text = pending.text.has_value() ? &pending.text.value() : nullptr;
    const std::string* label = pending.label.has_value() ? &pending.label.value() : nullptr;
    const std::string* value = pending.value.has_value() ? &pending.value.value() : nullptr;
    UiNode node(pending.node_id, parent, child_ids, automation_id, text, label, value,
                pending.role, pending.bounds, pending.enabled, pending.clickable,
                pending.editable, pending.focused, pending.selected, pending.checked,
                pending.scrollable);
    nodes.emplace_back(flutter::CustomEncodableValue(std::move(node)));
  }
  Log("TRACE", "dumpUi", "after Flutter node serialization", S_OK,
      ERROR_SUCCESS, true);
  Log("INFO", "dumpUi", "nodes=" + std::to_string(nodes.size()) +
                              "; generation=" + std::to_string(generation_));
  return UiSnapshot(generation_, nodes);
}

ErrorOr<ActionResult> DeviceKitLibPlugin::PerformElementAction(
    const std::string& node_id,
    int64_t requested_generation,
    const UiAction& action,
    const std::string* value) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (requested_generation != generation_) {
    return Failed<ActionResult>(OperationFailure(
        "performElementAction", "The UI snapshot is stale; dumpUi again"));
  }
  const auto iterator = elements_by_node_id_.find(node_id);
  if (iterator == elements_by_node_id_.end()) {
    return Failed<ActionResult>(OperationFailure(
        "performElementAction", "The UI node is not present in the current snapshot"));
  }
  const AutomationElement& element = iterator->second;
  HRESULT hr = E_NOTIMPL;
  switch (action) {
    case UiAction::kPress: {
      // Flutter's Windows semantics provider can acknowledge IInvokeProvider::Invoke
      // without dispatching the widget callback. Prefer an actual pointer click when
      // UIA provides bounds; this also follows the same path as a user click.
      if (ResolveTargetRoot("performElementAction", false)) {
        RECT rectangle{};
        const HRESULT bounds_hr = element->get_CurrentBoundingRectangle(&rectangle);
        const LONG width = rectangle.right - rectangle.left;
        const LONG height = rectangle.bottom - rectangle.top;
        Log("TRACE", "performElementAction",
            "press bounds; node=" + node_id + "; left=" +
                std::to_string(rectangle.left) + "; top=" +
                std::to_string(rectangle.top) + "; width=" +
                std::to_string(width) + "; height=" + std::to_string(height),
            bounds_hr, FAILED(bounds_hr) ? GetLastError() : ERROR_SUCCESS, true);
        if (SUCCEEDED(bounds_hr) && width > 0 && height > 0) {
          const double x = static_cast<double>(rectangle.left) + width / 2.0;
          const double y = static_cast<double>(rectangle.top) + height / 2.0;
          if (SendMouseClick(x, y)) {
            Log("INFO", "performElementAction",
                "node=" + node_id + "; action=press; method=coordinate; x=" +
                    std::to_string(x) + "; y=" + std::to_string(y),
                S_OK, ERROR_SUCCESS, true);
            return Success(true);
          }
          return Failed<ActionResult>(OperationFailure(
              "performElementAction", "SendInput mouse click failed", S_OK,
              GetLastError()));
        }
      }

      ComPtr<IUIAutomationInvokePattern> invoke;
      hr = element->GetCurrentPatternAs(UIA_InvokePatternId,
                                        IID_PPV_ARGS(invoke.GetAddressOf()));
      if (SUCCEEDED(hr)) {
        hr = invoke->Invoke();
        break;
      }
      ComPtr<IUIAutomationSelectionItemPattern> selection;
      hr = element->GetCurrentPatternAs(UIA_SelectionItemPatternId,
                                        IID_PPV_ARGS(selection.GetAddressOf()));
      if (SUCCEEDED(hr)) {
        hr = selection->Select();
        break;
      }
      ComPtr<IUIAutomationTogglePattern> toggle;
      hr = element->GetCurrentPatternAs(UIA_TogglePatternId,
                                        IID_PPV_ARGS(toggle.GetAddressOf()));
      if (SUCCEEDED(hr)) {
        hr = toggle->Toggle();
      }
      break;
    }
    case UiAction::kFocus:
      hr = element->SetFocus();
      break;
    case UiAction::kSetValue: {
      if (value == nullptr) {
        return Failed<ActionResult>(OperationFailure(
            "performElementAction", "setValue requires a value"));
      }
      ComPtr<IUIAutomationValuePattern> value_pattern;
      hr = element->GetCurrentPatternAs(UIA_ValuePatternId,
                                        IID_PPV_ARGS(value_pattern.GetAddressOf()));
      if (SUCCEEDED(hr)) {
        const std::wstring wide_value = Utf8ToWide(*value);
        BSTR bstr = SysAllocString(wide_value.c_str());
        if (bstr == nullptr) {
          hr = E_OUTOFMEMORY;
        } else {
          hr = value_pattern->SetValue(bstr);
          SysFreeString(bstr);
        }
      }
      break;
    }
    case UiAction::kScrollForward:
    case UiAction::kScrollBackward: {
      ComPtr<IUIAutomationScrollPattern> scroll;
      hr = element->GetCurrentPatternAs(UIA_ScrollPatternId,
                                        IID_PPV_ARGS(scroll.GetAddressOf()));
      if (SUCCEEDED(hr)) {
        const ScrollAmount amount = action == UiAction::kScrollForward
                                        ? ScrollAmount_LargeIncrement
                                        : ScrollAmount_LargeDecrement;
        hr = scroll->Scroll(ScrollAmount_NoAmount, amount);
      }
      break;
    }
  }
  if (FAILED(hr)) {
    return Failed<ActionResult>(OperationFailure(
        "performElementAction", "The requested UIA pattern/action is unavailable",
        hr, GetLastError()));
  }
  Log("INFO", "performElementAction", "node=" + node_id + "; action=" +
                                          std::to_string(static_cast<int>(action)));
  return Success(true);
}

bool DeviceKitLibPlugin::SendMouseClick(double x, double y) {
  if (SetCursorPos(static_cast<int>(std::lround(x)), static_cast<int>(std::lround(y))) ==
      FALSE) {
    return false;
  }
  INPUT inputs[2]{};
  inputs[0].type = INPUT_MOUSE;
  inputs[0].mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
  inputs[1].type = INPUT_MOUSE;
  inputs[1].mi.dwFlags = MOUSEEVENTF_LEFTUP;
  return SendInput(2, inputs, sizeof(INPUT)) == 2;
}

bool DeviceKitLibPlugin::SendMouseSwipe(double from_x, double from_y, double to_x,
                                        double to_y, int64_t duration_ms) {
  const int64_t bounded_duration = std::clamp<int64_t>(duration_ms, 0, 30000);
  if (SetCursorPos(static_cast<int>(std::lround(from_x)),
                   static_cast<int>(std::lround(from_y))) == FALSE) {
    return false;
  }
  INPUT down{};
  down.type = INPUT_MOUSE;
  down.mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
  if (SendInput(1, &down, sizeof(INPUT)) != 1) {
    return false;
  }
  const int steps = std::max<int>(1, static_cast<int>(bounded_duration / 16));
  for (int step = 1; step <= steps; ++step) {
    const double fraction = static_cast<double>(step) / static_cast<double>(steps);
    const int x = static_cast<int>(std::lround(from_x + (to_x - from_x) * fraction));
    const int y = static_cast<int>(std::lround(from_y + (to_y - from_y) * fraction));
    if (SetCursorPos(x, y) == FALSE) {
      return false;
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(
        bounded_duration == 0 ? 0 : bounded_duration / steps));
  }
  INPUT up{};
  up.type = INPUT_MOUSE;
  up.mi.dwFlags = MOUSEEVENTF_LEFTUP;
  return SendInput(1, &up, sizeof(INPUT)) == 1;
}

bool DeviceKitLibPlugin::SendKey(WORD virtual_key, bool with_control, bool with_alt) {
  std::vector<INPUT> inputs;
  if (with_control) {
    INPUT input{};
    input.type = INPUT_KEYBOARD;
    input.ki.wVk = VK_CONTROL;
    inputs.push_back(input);
  }
  if (with_alt) {
    INPUT input{};
    input.type = INPUT_KEYBOARD;
    input.ki.wVk = VK_MENU;
    inputs.push_back(input);
  }
  INPUT down{};
  down.type = INPUT_KEYBOARD;
  down.ki.wVk = virtual_key;
  inputs.push_back(down);
  INPUT up = down;
  up.ki.dwFlags = KEYEVENTF_KEYUP;
  inputs.push_back(up);
  if (with_alt) {
    INPUT input{};
    input.type = INPUT_KEYBOARD;
    input.ki.wVk = VK_MENU;
    input.ki.dwFlags = KEYEVENTF_KEYUP;
    inputs.push_back(input);
  }
  if (with_control) {
    INPUT input{};
    input.type = INPUT_KEYBOARD;
    input.ki.wVk = VK_CONTROL;
    input.ki.dwFlags = KEYEVENTF_KEYUP;
    inputs.push_back(input);
  }
  return SendInput(static_cast<UINT>(inputs.size()), inputs.data(), sizeof(INPUT)) ==
         static_cast<UINT>(inputs.size());
}

bool DeviceKitLibPlugin::SendUnicodeText(const std::wstring& text) {
  std::vector<INPUT> inputs;
  inputs.reserve(text.size() * 2);
  for (wchar_t character : text) {
    INPUT down{};
    down.type = INPUT_KEYBOARD;
    down.ki.wScan = static_cast<WORD>(character);
    down.ki.dwFlags = KEYEVENTF_UNICODE;
    inputs.push_back(down);
    INPUT up = down;
    up.ki.dwFlags = KEYEVENTF_UNICODE | KEYEVENTF_KEYUP;
    inputs.push_back(up);
  }
  return inputs.empty() ||
         SendInput(static_cast<UINT>(inputs.size()), inputs.data(), sizeof(INPUT)) ==
             static_cast<UINT>(inputs.size());
}

ErrorOr<ActionResult> DeviceKitLibPlugin::Tap(double x, double y) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!ResolveTargetRoot("tap", false)) {
    return Failed<ActionResult>(OperationFailure(
        "tap", "Unable to activate the target window before coordinate input"));
  }
  if (!SendMouseClick(x, y)) {
    return Failed<ActionResult>(OperationFailure(
        "tap", "SendInput mouse click failed", S_OK, GetLastError()));
  }
  Log("INFO", "tap", "x=" + std::to_string(x) + "; y=" + std::to_string(y));
  return Success(true);
}

ErrorOr<ActionResult> DeviceKitLibPlugin::Swipe(
    double from_x, double from_y, double to_x, double to_y, int64_t duration_ms) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!ResolveTargetRoot("swipe", false)) {
    return Failed<ActionResult>(OperationFailure(
        "swipe", "Unable to activate the target window before coordinate input"));
  }
  if (!SendMouseSwipe(from_x, from_y, to_x, to_y, duration_ms)) {
    return Failed<ActionResult>(OperationFailure(
        "swipe", "SendInput mouse swipe failed", S_OK, GetLastError()));
  }
  Log("INFO", "swipe", "from=" + std::to_string(from_x) + "," +
                               std::to_string(from_y) + "; to=" +
                               std::to_string(to_x) + "," + std::to_string(to_y) +
                               "; duration_ms=" + std::to_string(duration_ms));
  return Success(true);
}

ErrorOr<ActionResult> DeviceKitLibPlugin::TypeText(const std::string& text) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!ResolveTargetRoot("typeText", false)) {
    return Failed<ActionResult>(OperationFailure(
        "typeText", "Unable to activate the target window before keyboard input"));
  }
  const std::wstring wide_text = Utf8ToWide(text);
  if (!SendUnicodeText(wide_text)) {
    return Failed<ActionResult>(OperationFailure(
        "typeText", "SendInput Unicode keyboard input failed", S_OK, GetLastError()));
  }
  Log("INFO", "typeText", "characters=" + std::to_string(wide_text.size()));
  return Success(true);
}

ErrorOr<ActionResult> DeviceKitLibPlugin::PressBack() {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!ResolveTargetRoot("pressBack", false) || !SendKey(VK_ESCAPE)) {
    return Failed<ActionResult>(OperationFailure(
        "pressBack", "SendInput Escape key failed", S_OK, GetLastError()));
  }
  return Success(true);
}

ErrorOr<ActionResult> DeviceKitLibPlugin::PressHome() {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!ResolveTargetRoot("pressHome", false) || !SendKey(VK_HOME)) {
    return Failed<ActionResult>(OperationFailure(
        "pressHome", "SendInput Home key failed", S_OK, GetLastError()));
  }
  return Success(true);
}

ErrorOr<std::vector<uint8_t>> DeviceKitLibPlugin::Screenshot() {
  std::lock_guard<std::mutex> lock(mutex_);
  const auto start = std::chrono::steady_clock::now();
  ErrorOr<std::vector<uint8_t>> result = CaptureDesktopPng();
  if (result.has_error()) {
    Log("ERROR", "screenshot", result.error().message(), S_OK, GetLastError(), true);
    return result;
  }
  const auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
                           std::chrono::steady_clock::now() - start)
                           .count();
  Log("INFO", "screenshot", "bytes=" + std::to_string(result.value().size()) +
                                "; elapsed_ms=" + std::to_string(elapsed));
  return result;
}

ErrorOr<ActionResult> DeviceKitLibPlugin::RequestScreenCapture() {
  std::lock_guard<std::mutex> lock(mutex_);
  HDC dc = GetDC(nullptr);
  if (dc == nullptr) {
    return Failed<ActionResult>(OperationFailure(
        "requestScreenCapture", "The desktop screen is not accessible", S_OK,
        GetLastError()));
  }
  ReleaseDC(nullptr, dc);
  Log("INFO", "requestScreenCapture",
      "Windows desktop capture does not require a runtime consent dialog");
  return Success(false);
}

ErrorOr<std::optional<std::string>> DeviceKitLibPlugin::GetClipboard() {
  std::lock_guard<std::mutex> lock(mutex_);
  if (OpenClipboard(nullptr) == FALSE) {
    return Failed<std::optional<std::string>>(OperationFailure(
        "getClipboard", "OpenClipboard failed", S_OK, GetLastError()));
  }
  HANDLE handle = GetClipboardData(CF_UNICODETEXT);
  if (handle == nullptr) {
    CloseClipboard();
    Log("INFO", "getClipboard", "Clipboard has no Unicode text");
    return std::optional<std::string>();
  }
  const wchar_t* text = static_cast<const wchar_t*>(GlobalLock(handle));
  if (text == nullptr) {
    CloseClipboard();
    return Failed<std::optional<std::string>>(OperationFailure(
        "getClipboard", "GlobalLock clipboard data failed", S_OK, GetLastError()));
  }
  const std::string result = WideToUtf8(text);
  GlobalUnlock(handle);
  CloseClipboard();
  Log("INFO", "getClipboard", "characters=" + std::to_string(result.size()));
  return std::optional<std::string>(result);
}

ErrorOr<ActionResult> DeviceKitLibPlugin::SetClipboard(const std::string& text) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (OpenClipboard(nullptr) == FALSE) {
    return Failed<ActionResult>(OperationFailure(
        "setClipboard", "OpenClipboard failed", S_OK, GetLastError()));
  }
  EmptyClipboard();
  const std::wstring wide_text = Utf8ToWide(text);
  const size_t bytes = (wide_text.size() + 1) * sizeof(wchar_t);
  HGLOBAL memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
  if (memory == nullptr) {
    CloseClipboard();
    return Failed<ActionResult>(OperationFailure(
        "setClipboard", "GlobalAlloc clipboard data failed", S_OK, GetLastError()));
  }
  void* destination = GlobalLock(memory);
  if (destination == nullptr) {
    GlobalFree(memory);
    CloseClipboard();
    return Failed<ActionResult>(OperationFailure(
        "setClipboard", "GlobalLock clipboard data failed", S_OK, GetLastError()));
  }
  memcpy(destination, wide_text.c_str(), bytes);
  GlobalUnlock(memory);
  if (SetClipboardData(CF_UNICODETEXT, memory) == nullptr) {
    GlobalFree(memory);
    CloseClipboard();
    return Failed<ActionResult>(OperationFailure(
        "setClipboard", "SetClipboardData failed", S_OK, GetLastError()));
  }
  CloseClipboard();
  Log("INFO", "setClipboard", "characters=" + std::to_string(wide_text.size()));
  return Success(false);
}

std::string DeviceKitLibPlugin::OperationFailure(const char* operation,
                                                 const std::string& detail,
                                                 HRESULT hr,
                                                 DWORD win32_error) {
  const std::string message = FormatOperationFailure(detail, hr, win32_error);
  Log("ERROR", operation, message, hr, win32_error, true);
  return message;
}

ActionResult DeviceKitLibPlugin::Success(bool ui_changed) const {
  return ActionResult(true, ui_changed);
}

ActionResult DeviceKitLibPlugin::Failure(const std::string& message) const {
  return ActionResult(false, &message, false);
}

void DeviceKitLibPlugin::Log(const char* level, const char* operation,
                             const std::string& message, HRESULT hr,
                             DWORD win32_error, bool force) {
  if (!force && !logging_enabled_) {
    return;
  }
  SYSTEMTIME time{};
  GetLocalTime(&time);
  const std::string target =
      target_name_.empty() ? std::string("<none>") : WideToUtf8(target_name_);
  std::ostringstream line;
  line << '[' << std::setfill('0') << std::setw(4) << time.wYear << '-'
       << std::setw(2) << time.wMonth << '-' << std::setw(2) << time.wDay << 'T'
       << std::setw(2) << time.wHour << ':' << std::setw(2) << time.wMinute << ':'
       << std::setw(2) << time.wSecond << '.' << std::setw(3) << time.wMilliseconds
       << "] [" << level << "] [session=" << session_id_ << "] [operation="
       << operation << "] [thread=" << GetCurrentThreadId() << "] [target="
       << target << "] [pid=" << target_process_id_ << "] [generation="
       << generation_ << "] "
       << message;
  if (FAILED(hr)) {
    line << " [hresult=" << HResultString(hr) << ']';
  }
  if (win32_error != ERROR_SUCCESS) {
    line << " [win32=" << win32_error << "]";
  }
  line << '\n';
  const std::string output = line.str();
  OutputDebugStringA(output.c_str());
  if (!log_path_.empty()) {
    std::ofstream file(log_path_, std::ios::app | std::ios::binary);
    if (file.is_open()) {
      file << output;
      file.flush();
    }
  }
}

}  // namespace device_kit_lib

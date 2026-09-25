#ifndef FLUTTER_PLUGIN_DEVICE_KIT_LIB_PLUGIN_H_
#define FLUTTER_PLUGIN_DEVICE_KIT_LIB_PLUGIN_H_

#include <flutter/encodable_value.h>
#include <flutter/plugin_registrar_windows.h>

#include <uiautomation.h>
#include <wrl/client.h>

#include <cstdint>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <vector>

#include "messages.g.h"

namespace device_kit_lib {

class DeviceKitLibPlugin final : public flutter::Plugin, public DeviceKitHostApi {
 public:
  static void RegisterWithRegistrar(flutter::PluginRegistrarWindows *registrar);

  DeviceKitLibPlugin();

  virtual ~DeviceKitLibPlugin();

  // Disallow copy and assign.
  DeviceKitLibPlugin(const DeviceKitLibPlugin&) = delete;
  DeviceKitLibPlugin& operator=(const DeviceKitLibPlugin&) = delete;

  std::optional<FlutterError> Initialize(const DriverConfig& config) override;
  std::optional<FlutterError> Dispose() override;
  ErrorOr<DeviceInfo> GetDeviceInfo() override;
  ErrorOr<ActionResult> OpenAccessibilitySettings() override;
  ErrorOr<ActionResult> LaunchApp(const std::string& package_name) override;
  ErrorOr<UiSnapshot> DumpUi() override;
  ErrorOr<ActionResult> PerformElementAction(
      const std::string& node_id,
      int64_t generation,
      const UiAction& action,
      const std::string* value) override;
  ErrorOr<ActionResult> Tap(double x, double y) override;
  ErrorOr<ActionResult> Swipe(
      double from_x,
      double from_y,
      double to_x,
      double to_y,
      int64_t duration_ms) override;
  ErrorOr<ActionResult> TypeText(const std::string& text) override;
  ErrorOr<ActionResult> PressBack() override;
  ErrorOr<ActionResult> PressHome() override;
  ErrorOr<std::vector<uint8_t>> Screenshot() override;
  ErrorOr<ActionResult> RequestScreenCapture() override;
  ErrorOr<std::optional<std::string>> GetClipboard() override;
  ErrorOr<ActionResult> SetClipboard(const std::string& text) override;

 private:
  using AutomationElement = Microsoft::WRL::ComPtr<IUIAutomationElement>;

  struct PendingNode {
    std::string node_id;
    std::optional<std::string> parent_node_id;
    AutomationElement element;
    std::vector<std::string> child_node_ids;
    std::optional<std::string> automation_id;
    std::optional<std::string> text;
    std::optional<std::string> label;
    std::optional<std::string> value;
    std::string role;
    RectData bounds{0.0, 0.0, 0.0, 0.0};
    bool enabled = true;
    bool clickable = false;
    bool editable = false;
    bool focused = false;
    bool selected = false;
    bool checked = false;
    bool scrollable = false;
  };

  bool EnsureComInitialized(const char* operation);
  bool EnsureAutomation(const char* operation);
  bool ResolveTargetRoot(const char* operation, bool retry);
  bool ActivateTargetWindow(const char* operation);
  std::optional<std::string> AppendElementTree(
      const AutomationElement& element,
      const std::optional<std::string>& parent_node_id,
      Microsoft::WRL::ComPtr<IUIAutomationTreeWalker> walker,
      const char* operation);
  bool ReadElement(const AutomationElement& element, PendingNode* node);
  bool SendKey(WORD virtual_key, bool with_control = false, bool with_alt = false);
  bool SendUnicodeText(const std::wstring& text);
  bool SendMouseClick(double x, double y);
  bool SendMouseSwipe(double from_x, double from_y, double to_x, double to_y,
                      int64_t duration_ms);
  std::optional<uint32_t> FindProcessId(const std::wstring& target) const;
  std::optional<uint32_t> StartTarget(const std::wstring& target);
  std::string OperationFailure(const char* operation, const std::string& detail,
                               HRESULT hr = S_OK, DWORD win32_error = ERROR_SUCCESS);
  ActionResult Success(bool ui_changed) const;
  ActionResult Failure(const std::string& message) const;
  void Log(const char* level, const char* operation, const std::string& message,
           HRESULT hr = S_OK, DWORD win32_error = ERROR_SUCCESS, bool force = false);
  void ResetTarget();

  std::mutex mutex_;
  Microsoft::WRL::ComPtr<IUIAutomation> automation_;
  Microsoft::WRL::ComPtr<IUIAutomationElement> target_root_;
  // Flutter's Windows UIA provider can omit semantic nodes from Control View.
  // Raw View preserves the complete provider fragment; callers still filter
  // the serialized nodes by role/name/id.
  Microsoft::WRL::ComPtr<IUIAutomationTreeWalker> raw_view_walker_;
  std::map<std::string, AutomationElement> elements_by_node_id_;
  std::vector<PendingNode> pending_nodes_;
  std::string session_id_ = "default";
  std::string log_path_;
  std::wstring target_name_;
  uint32_t target_process_id_ = 0;
  int64_t generation_ = 0;
  bool initialized_ = false;
  bool logging_enabled_ = false;
  PVOID crash_log_handler_ = nullptr;
  bool com_initialized_ = false;
  HRESULT com_init_result_ = E_NOTIMPL;
  DWORD com_thread_id_ = 0;
};

}  // namespace device_kit_lib

#endif  // FLUTTER_PLUGIN_DEVICE_KIT_LIB_PLUGIN_H_

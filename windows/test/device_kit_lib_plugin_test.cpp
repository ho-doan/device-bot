#include <gtest/gtest.h>
#include <windows.h>

#include <memory>
#include <string>

#include "device_kit_lib_plugin.h"

namespace device_kit_lib {
namespace test {

namespace {

}  // namespace

TEST(DeviceKitLibPlugin, InitializesWindowsUiAutomationBackend) {
  DeviceKitLibPlugin plugin;
  const std::optional<FlutterError> initialize_error =
      plugin.Initialize(DriverConfig("windows-unit-test", true));
  ASSERT_FALSE(initialize_error.has_value())
      << (initialize_error.has_value() ? initialize_error->message() : "");

  const ErrorOr<DeviceInfo> info = plugin.GetDeviceInfo();
  ASSERT_FALSE(info.has_error())
      << (info.has_error() ? info.error().message() : "");
  EXPECT_EQ(info.value().platform(), "Windows");

  EXPECT_FALSE(plugin.Dispose().has_value());
}

}  // namespace test
}  // namespace device_kit_lib

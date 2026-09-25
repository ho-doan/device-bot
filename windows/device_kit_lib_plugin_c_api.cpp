#include "include/device_kit_lib/device_kit_lib_plugin_c_api.h"

#include <flutter/plugin_registrar_windows.h>

#include "device_kit_lib_plugin.h"

void DeviceKitLibPluginCApiRegisterWithRegistrar(
    FlutterDesktopPluginRegistrarRef registrar) {
  device_kit_lib::DeviceKitLibPlugin::RegisterWithRegistrar(
      flutter::PluginRegistrarManager::GetInstance()
          ->GetRegistrar<flutter::PluginRegistrarWindows>(registrar));
}

//
//  Generated file. Do not edit.
//

// clang-format off

#include "generated_plugin_registrant.h"

#include <device_kit_lib/device_kit_lib_plugin.h>

void fl_register_plugins(FlPluginRegistry* registry) {
  g_autoptr(FlPluginRegistrar) device_kit_lib_registrar =
      fl_plugin_registry_get_registrar_for_plugin(registry, "DeviceKitLibPlugin");
  device_kit_lib_plugin_register_with_registrar(device_kit_lib_registrar);
}

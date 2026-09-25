import 'package:device_kit_lib/device_kit_lib.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:patrol/patrol.dart';

const _exampleAppPackage = 'com.example.example_app';

void main() {
  patrolTest('controller drives example_app through the shared Dart API', (
    $,
  ) async {
    final service = AutomationService(
      AndroidDriver(),
      defaultTimeout: const Duration(seconds: 15),
      pollInterval: const Duration(milliseconds: 150),
    );
    await service.start(sessionId: 'patrol-android-controller-e2e');
    try {
      await _ensureAccessibilityService($, service);
      final deviceInfo = await service.driver.getDeviceInfo();
      final androidMajor = int.tryParse(
        deviceInfo.osVersion?.split('.').first ?? '',
      );
      if (androidMajor != null && androidMajor < 11) {
        await service.requestScreenCapture();
        await _approveMediaProjectionConsent($);
      }

      // Patrol is used only for the native MediaProjection consent dialog.
      // Target-app discovery and every target-app action stay in the shared
      // device_kit_lib Dart API.
      await service.launchApp(_exampleAppPackage);
      await service.waitFor(
        By.all(<By>[By.id('counter_value'), By.value('0')]),
      );
      final button = await service.waitFor(By.id('increment_button'));
      await button.tap();
      await service.waitFor(
        By.all(<By>[By.id('counter_value'), By.value('1')]),
      );
      expect((await service.screenshot()).isNotEmpty, isTrue);
    } finally {
      await service.stop();
    }
  });
}

Future<void> _ensureAccessibilityService(
  PatrolIntegrationTester $,
  AutomationService service,
) async {
  try {
    await service.dumpUi();
    return;
  } on Object {
    // AccessibilityService is a special Settings permission, not a runtime
    // permission. Continue into the test-only Settings flow when disabled.
  }

  await $.platform.android.openPlatformApp(
    androidAppId: 'com.android.settings',
  );
  await _tapWithScroll($, <AndroidSelector>[
    const AndroidSelector(text: 'Accessibility'),
    const AndroidSelector(textContains: 'Accessibility'),
  ]);
  await _tapFirst($, <AndroidSelector>[
    const AndroidSelector(text: 'Installed services'),
    const AndroidSelector(text: 'Installed apps'),
    const AndroidSelector(text: 'Downloaded apps'),
  ]);
  await _tapFirst($, <AndroidSelector>[
    const AndroidSelector(text: 'Device Kit Accessibility'),
    const AndroidSelector(textContains: 'Device Kit Accessibility'),
  ]);

  try {
    await _tapFirst($, <AndroidSelector>[
      const AndroidSelector(
        contentDescriptionContains: 'Device Kit Accessibility',
        isChecked: false,
      ),
      const AndroidSelector(
        resourceName: 'com.android.settings:id/switch_widget',
        isChecked: false,
      ),
      const AndroidSelector(
        resourceName: 'android:id/switch_widget',
        isChecked: false,
      ),
    ]);
  } on Object {
    // The service was already enabled, so there is no switch to turn on.
  }

  try {
    await _tapFirst($, <AndroidSelector>[
      const AndroidSelector(text: 'Allow'),
      const AndroidSelector(text: 'OK'),
    ]);
  } on Object {
    // Some Android images enable the service without a confirmation dialog.
  }

  // Settings path: service detail -> installed services -> Accessibility ->
  // Settings home -> controller app.
  for (var index = 0; index < 4; index++) {
    await $.platform.android.pressBack();
  }
}

Future<void> _tapFirst(
  PatrolIntegrationTester $,
  List<AndroidSelector> selectors,
) async {
  for (final selector in selectors) {
    try {
      await $.platform.android.tap(selector);
      return;
    } on Object {
      // Try the next Android/OEM selector variant.
    }
  }
  throw StateError('No native selector matched in the permission setup flow.');
}

Future<void> _tapWithScroll(
  PatrolIntegrationTester $,
  List<AndroidSelector> selectors,
) async {
  for (var attempt = 0; attempt < 5; attempt++) {
    try {
      await _tapFirst($, selectors);
      return;
    } on Object {
      await $.platform.android.swipe(
        from: const Offset(0.5, 0.85),
        to: const Offset(0.5, 0.3),
      );
    }
  }
  throw StateError('Accessibility Settings entry was not found.');
}

Future<void> _approveMediaProjectionConsent(PatrolIntegrationTester $) async {
  // The exact label varies by Android version and OEM. This is intentionally
  // test-only and does not leak into the plugin runtime API.
  for (final label in <String>['Start now', 'Start Now', 'Allow']) {
    try {
      await $.platform.android.tap(AndroidSelector(text: label));
      return;
    } on Object {
      // Try the next system-label variant.
    }
  }
  throw StateError(
    'MediaProjection consent dialog was not found. Approve it manually or '
    'update the Patrol selector for this Android/OEM image.',
  );
}

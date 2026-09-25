# Android E2E evidence

Captured from the connected Android device during the Device Kit scenario:

1. `01_controller_ready.png` — controller app before starting the kit.
2. `02_accessibility_settings.png` — Android Accessibility settings opened for
   enabling the Device Kit service.
3. `03_counter_0.png` — independent `example_app` with `counter_value = 0`.
4. `04_counter_1.png` — `example_app` after tapping `increment_button`.
5. `05_screenshot.png` — final screen-capture artifact.

The iOS XCTest evidence is also captured here:

1. `06_ios_xctest_counter_1.png` — `example_app` after XCTest presses
   `increment_button` and observes `counter_value = 1` on an iPhone Air
   simulator.

The macOS AX evidence is also captured here:

1. `07_macos_ax_counter_1.png` — `example_app` after the macOS AX session
   launches the target and the counter is at `1`; the final increment was
   performed at the target's visible control because Flutter's macOS AX tree
   did not expose `Semantics.identifier` in this run.

The macOS controller screenshot API was also exercised; it returned a PNG
payload from `CGDisplayCreateImage` after the AX session was initialized.

The Android and iOS interactions use the shared Dart `AutomationService` API;
no Flutter Finder, CSS, or XPath is used. The macOS launch, AX dump, and
screenshot API were verified through the same service, while semantic
`By.id('increment_button')` is currently blocked by the Flutter macOS
semantics bridge not being exposed when VoiceOver is inactive.

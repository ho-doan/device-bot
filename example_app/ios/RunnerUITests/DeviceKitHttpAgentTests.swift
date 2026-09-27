import device_kit_lib_xctest
import XCTest

/// Keeps the DeviceKit XCTest agent alive on a simulator or paired iPhone.
///
/// The host creates a short-lived HTTP bridge and this UI test polls it until
/// the host sends `stop`. This lets the same XCTest bundle control whichever
/// application bundle the host subsequently launches.
final class DeviceKitHttpAgentTests: XCTestCase {
  func testServe() {
    let hosts = (ProcessInfo.processInfo.environment["TEST_RUNNER_DEVICE_KIT_HOSTS"] ?? "")
      .split(separator: ",")
      .map { String($0).trimmingCharacters(in: .whitespacesAndNewlines) }
      .filter { !$0.isEmpty }
    let port = Int(ProcessInfo.processInfo.environment["TEST_RUNNER_DEVICE_KIT_PORT"] ?? "")
      ?? 18_787

    XCTAssertFalse(hosts.isEmpty, "TEST_RUNNER_DEVICE_KIT_HOSTS is required")
    guard !hosts.isEmpty else { return }
    DeviceKitXCTestHostAgent(hosts: hosts, port: port).run()
  }
}

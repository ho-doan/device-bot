import device_kit_lib_xctest
import XCTest

final class MobileMcpRealRunnerUITests: XCTestCase {
  func testServe() {
    let environment = ProcessInfo.processInfo.environment
    let hosts = (environment["MOBILE_MCP_HOSTS"] ?? "")
      .split(separator: ",")
      .map { String($0) }
      .filter { !$0.isEmpty }
    let port = Int(environment["MOBILE_MCP_PORT"] ?? "18787") ?? 18787
    let token = environment["MOBILE_MCP_TOKEN"]
    XCTAssertFalse(hosts.isEmpty, "MOBILE_MCP_HOSTS is required")
    DeviceKitXCTestHostAgent(hosts: hosts, port: port, token: token).run()
  }
}

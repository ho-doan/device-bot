import Foundation
import XCTest

final class MobileMcpSimulatorRunnerUITests: XCTestCase {
  func testServe() {
    let environment = ProcessInfo.processInfo.environment
    let hosts = (environment["MOBILE_MCP_HOSTS"] ?? "127.0.0.1")
      .split(separator: ",")
      .map(String.init)
      .filter { !$0.isEmpty }
    let port = Int(environment["MOBILE_MCP_PORT"] ?? "18788") ?? 18788
    let token = environment["MOBILE_MCP_TOKEN"]
    DeviceKitXCTestHostAgent(hosts: hosts, port: port, token: token).run()
  }
}

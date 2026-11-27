import device_kit_lib_xctest
import Foundation
import Network
import UIKit
import XCTest

final class MobileMcpRealRunnerUITests: XCTestCase {
  /// Kept alive so the Bonjour browse still triggers Local Network privacy
  /// after `requestLocalNetworkPermission` returns into the command loop.
  private var localNetworkBrowser: NWBrowser?

  func testSnapshotRetriesOnlyAStaleElementRead() throws {
    var reads = 0
    let tree = try readUiSnapshot {
      reads += 1
      if reads == 1 {
        throw NSError(domain: "com.apple.dt.xctest.ui-testing.error", code: 10008)
      }
      return ["current element"]
    }
    XCTAssertEqual(tree, ["current element"])
    XCTAssertEqual(reads, 2)

    reads = 0
    XCTAssertThrowsError(try readUiSnapshot {
      reads += 1
      throw NSError(domain: "com.apple.dt.xctest.ui-testing.error", code: 44)
    })
    XCTAssertEqual(reads, 1)
  }

  func testTerminateRejectsAnUnselectedApplication() {
    let agent = MobileMcpRealHostAgent(hosts: [], port: 0, token: nil)
    let result = agent.execute(Command(
      id: UUID().uuidString, method: "terminate", params: ["bundleId": "com.example.unselected"]
    ))
    XCTAssertEqual(result["ok"] as? Bool, false)
  }

  /// Runs the real command dispatcher on a device without the LAN bridge.
  /// The fixture is Orbit; opt in with DEVICE_BOT_REGRESSION_APP_ID.
  func testFocusedInputAndWhitespace() throws {
    continueAfterFailure = false
    guard let appId = ProcessInfo.processInfo.environment["DEVICE_BOT_REGRESSION_APP_ID"] else {
      throw XCTSkip("Set DEVICE_BOT_REGRESSION_APP_ID to the installed Orbit fixture")
    }
    let agent = MobileMcpRealHostAgent(hosts: [], port: 0, token: nil)
    func send(_ method: String, _ params: [String: Any] = [:]) throws -> [String: Any] {
      let body = agent.execute(Command(id: UUID().uuidString, method: method, params: params))
      let ok = try XCTUnwrap(body["ok"] as? Bool)
      if !ok {
        XCTContext.runActivity(named: "Native UI after \(method) failure") { activity in
          let keyboard = XCUIApplication(bundleIdentifier: appId).keyboards.firstMatch.debugDescription
          let tree = XCTAttachment(string: keyboard)
          tree.lifetime = .keepAlways
          activity.add(tree)
          activity.add(XCTAttachment(screenshot: XCUIScreen.main.screenshot()))
        }
      }
      XCTAssertTrue(ok, "\(method): \(body["message"] ?? "failed")")
      guard ok else { throw NSError(domain: "DeviceBotRegression", code: 1) }
      return body
    }
    func field(_ label: String) throws -> [String: Any] {
      let body = try send("dump", ["filter": label, "elementType": "textField", "maxResults": 2])
      let elements = try XCTUnwrap(body["elements"] as? [[String: Any]])
      XCTAssertEqual(elements.count, 1, "Expected exactly one \(label) field")
      return try XCTUnwrap(elements.first)
    }

    _ = try send("launch", ["bundleId": appId])
    defer { _ = try? send("stop") }
    let email = "  ALEX@ORBIT.DEMO  "
    let password = " Orbit123! "
    _ = try send("tap-element", ["filter": "Email address"])
    _ = try send("text", ["text": email])
    XCTAssertEqual(try field("Email address")["value"] as? String, email)
    _ = try send("ime-action", ["action": "next"])
    XCTAssertEqual(try field("Password")["focused"] as? Bool, true)
    _ = try send("text", ["text": password])
    _ = try send("tap-element", ["filter": "Show password"])
    XCTAssertEqual(try field("Email address")["value"] as? String, email)
    XCTAssertEqual(try field("Password")["value"] as? String, password)
    _ = try send("ime-action", ["action": "done"])

    let banner = "Email or password is incorrect. Try the demo account below."
    let deadline = Date().addingTimeInterval(8)
    var found = false
    repeat {
      let body = try send("dump", ["filter": banner, "maxResults": 2])
      found = (body["elements"] as? [[String: Any]])?.contains { $0["label"] as? String == banner } ?? false
      if !found { RunLoop.current.run(until: Date().addingTimeInterval(0.25)) }
    } while !found && Date() < deadline
    XCTAssertTrue(found, "Expected the validation banner from the native Flutter tree")
  }

  func testServe() {
    let environment = ProcessInfo.processInfo.environment
    let hosts = (environment["DEVICE_BOT_HOSTS"] ?? "")
      .split(separator: ",")
      .map(String.init)
      .filter { !$0.isEmpty }
    let port = Int(environment["DEVICE_BOT_PORT"] ?? "18787") ?? 18787
    let token = environment["DEVICE_BOT_TOKEN"]
    guard requestLocalNetworkPermission(host: hosts.first, port: port) else { return }
    MobileMcpRealHostAgent(hosts: hosts, port: port, token: token).run()
  }

  /// Ask iOS for Local Network access before the command loop. A first run
  /// shows the system sheet; a previous denial does not, so this opens
  /// Settings on the Local Network page and enables Device Bot there.
  private func requestLocalNetworkPermission(host: String?, port: Int) -> Bool {
    guard let host, let url = bridgeUrl(host: host, port: port, path: "/next") else {
      XCTFail("A reachable Mac bridge address is required for Local Network permission")
      return false
    }
    // XCTest may run its runner in the background. iOS will reject the first
    // LAN request without showing a permission alert unless the runner itself
    // is in the foreground (activating the host/target app is not sufficient).
    guard let runnerBundleId = Bundle.main.bundleIdentifier,
          runnerBundleId.hasSuffix(".xctrunner") else {
      XCTFail("Cannot identify the XCTest runner for Local Network permission")
      return false
    }
    let runner = XCUIApplication(bundleIdentifier: runnerBundleId)
    runner.activate()
    guard runner.wait(for: .runningForeground, timeout: 10) else {
      XCTFail("XCTest runner did not enter the foreground before Local Network access")
      return false
    }
    print("Device Bot XCTest runner is in the foreground; requesting Local Network access")
    triggerLocalNetworkAccess(url: url)
    if waitForLocalNetworkPermission(url: url, timeout: 12) {
      print("Device Bot Local Network permission allowed")
      return true
    }

    print("Device Bot opening Settings → Privacy & Security → Local Network")
    let settings = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
    let opened = openLocalNetworkSettings(settings)

    let deadline = Date().addingTimeInterval(90)
    while Date() < deadline {
      if waitForLocalNetworkPermission(url: url, timeout: 1) {
        print("Device Bot Local Network permission allowed")
        XCUIDevice.shared.press(.home)
        return true
      }
      RunLoop.current.run(until: Date().addingTimeInterval(1))
    }
    if opened {
      print("Device Bot Local Network still blocked. Settings is open on Local Network; turn on Device Bot.")
    } else {
      print("Device Bot could not open Local Network settings. Turn on Device Bot under Settings → Privacy & Security → Local Network.")
    }
    XCTFail("Local Network permission is required before starting the Device Bot command loop")
    return false
  }

  private func triggerLocalNetworkAccess(url: URL) {
    URLSession.shared.dataTask(with: url).resume()
    let browser = NWBrowser(for: .bonjour(type: "_http._tcp", domain: nil), using: .tcp)
    localNetworkBrowser = browser
    browser.start(queue: .main)
  }

  private func waitForLocalNetworkPermission(url: URL, timeout: TimeInterval) -> Bool {
    // The system dialog belongs to the user. Wait for their choice rather than
    // tapping a generic Allow button or changing a privacy switch for them.
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
      if !localNetworkProhibited(url) { return true }
      RunLoop.current.run(until: Date().addingTimeInterval(0.4))
    }
    return false
  }

  /// `true` only when iOS is refusing the LAN path. A refused TCP connection
  /// means the permission is already granted and the bridge is simply down.
  private func localNetworkProhibited(_ url: URL) -> Bool {
    var request = URLRequest(url: url)
    request.httpMethod = "GET"
    request.timeoutInterval = 4
    var prohibited = false
    let semaphore = DispatchSemaphore(value: 0)
    URLSession.shared.dataTask(with: request) { _, response, error in
      defer { semaphore.signal() }
      if response != nil { return }
      guard let error = error as? URLError else { return }
      let description = error.localizedDescription
      prohibited = error.code == .notConnectedToInternet
        || error.code == .dataNotAllowed
        || description.localizedCaseInsensitiveContains("local network")
        || description.localizedCaseInsensitiveContains("offline")
    }.resume()
    _ = semaphore.wait(timeout: .now() + 6)
    return prohibited
  }

  private func openLocalNetworkSettings(_ settings: XCUIApplication) -> Bool {
    let privacy = ["Privacy & Security", "Privacy", "Quyền riêng tư và Bảo mật", "Quyền riêng tư & Bảo mật"]
    let localNetwork = ["Local Network", "Mạng cục bộ"]
    if launchSettings(settings),
       tapSettingsRow(in: settings, labels: privacy),
       tapSettingsRow(in: settings, labels: localNetwork) {
      return true
    }
    return openLocalNetworkViaSearch(settings)
  }

  private func launchSettings(_ settings: XCUIApplication) -> Bool {
    settings.terminate()
    settings.launch()
    return settings.wait(for: .runningForeground, timeout: 8)
  }

  private func openLocalNetworkViaSearch(_ settings: XCUIApplication) -> Bool {
    for query in ["Local Network", "Mạng cục bộ"] {
      guard launchSettings(settings) else { continue }
      let search = settings.searchFields.firstMatch
      guard search.waitForExistence(timeout: 4) else { continue }
      search.tap()
      search.typeText(query)
      if tapSettingsRow(in: settings, labels: [query], swipes: 1) {
        return true
      }
    }
    return false
  }

  private func tapSettingsRow(in settings: XCUIApplication, labels: [String], swipes: Int = 4) -> Bool {
    let predicate = NSPredicate(format: "label IN %@", labels)
    for _ in 0...swipes {
      let cell = settings.cells.matching(predicate).firstMatch
      if cell.exists && cell.isHittable {
        cell.tap()
        return true
      }
      let button = settings.buttons.matching(predicate).firstMatch
      if button.exists && button.isHittable {
        button.tap()
        return true
      }
      settings.swipeUp()
    }
    return false
  }

  private func enableDeviceBotLocalNetworkSwitch(in settings: XCUIApplication) -> Bool {
    let names = ["Device Bot", "MobileMcpRealRunnerUITests-Runner", "MobileMcpRealRunnerUITests"]
    let predicate = NSPredicate(format: "label IN %@", names)
    for _ in 0...4 {
      let toggle = settings.switches.matching(predicate).firstMatch
      if toggle.exists && toggle.isHittable {
        if !switchIsOn(toggle) {
          toggle.tap()
        }
        print("Device Bot Local Network switch enabled")
        return true
      }
      settings.swipeUp()
    }
    print("Device Bot is not listed yet. Turn on Device Bot on the open Local Network page.")
    return false
  }

  private func switchIsOn(_ toggle: XCUIElement) -> Bool {
    switch toggle.value {
    case let number as NSNumber:
      return number.intValue == 1
    case let text as String:
      return text == "1"
    default:
      return false
    }
  }
}

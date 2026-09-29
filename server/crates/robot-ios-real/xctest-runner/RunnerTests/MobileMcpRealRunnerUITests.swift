import device_kit_lib_xctest
import Foundation
import UIKit
import XCTest

final class MobileMcpRealRunnerUITests: XCTestCase {
  func testServe() {
    let environment = ProcessInfo.processInfo.environment
    let hosts = (environment["MOBILE_MCP_HOSTS"] ?? "")
      .split(separator: ",")
      .map(String.init)
      .filter { !$0.isEmpty }
    let port = Int(environment["MOBILE_MCP_PORT"] ?? "18787") ?? 18787
    let token = environment["MOBILE_MCP_TOKEN"]
    MobileMcpRealHostAgent(hosts: hosts, port: port, token: token).run()
  }
}

/// XCTest-side command host for real devices.
///
/// The simulator runner has native filtered dumps and tap-element actions. The
/// real-device runner keeps the same command contract locally so the Rust
/// driver does not need to materialize the entire Safari tree for every tap.
private final class MobileMcpRealHostAgent {
  private let driver = DeviceKitXCTestDriver()
  private let hosts: [String]
  private let port: Int
  private let token: String?
  private var currentBundleId: String?
  private var running = true

  init(hosts: [String], port: Int, token: String?) {
    self.hosts = hosts.filter { !$0.isEmpty }
    self.port = port
    self.token = token?.isEmpty == false ? token : nil
  }

  func run() {
    while running {
      guard let command = nextCommand() else {
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        continue
      }
      postResult(id: command.id, body: execute(command))
      if command.method == "stop" { running = false }
    }
  }

  private func execute(_ command: Command) -> [String: Any] {
    switch command.method {
    case "health":
      return ["ok": true, "bundleId": currentBundleId ?? ""]
    case "launch":
      guard let bundleId = string(command.params["bundleId"]), !bundleId.isEmpty else {
        return ["ok": false, "message": "bundleId is required"]
      }
      driver.initialize(bundleIdentifier: bundleId)
      currentBundleId = bundleId
      driver.launch()
      return ["ok": true, "message": "app_launched"]
    case "openUrl":
      guard let rawUrl = string(command.params["url"]),
            let url = URL(string: rawUrl),
            ["http", "https"].contains(url.scheme?.lowercased()) else {
        return ["ok": false, "message": "invalid_http_url"]
      }
      let appId = string(command.params["appId"])
      let targetUrl: URL
      switch appId {
      case nil, "":
        targetUrl = url
      case "com.google.chrome.ios":
        guard var components = URLComponents(url: url, resolvingAgainstBaseURL: false) else {
          return ["ok": false, "message": "invalid_chrome_url"]
        }
        components.scheme = url.scheme?.lowercased() == "https" ? "googlechromes" : "googlechrome"
        guard let chromeUrl = components.url else {
          return ["ok": false, "message": "invalid_chrome_url"]
        }
        targetUrl = chromeUrl
      default:
        return [
          "ok": false,
          "message": "unsupported_browser_bundle; use com.google.chrome.ios or omit appId",
        ]
      }
      DispatchQueue.main.async { UIApplication.shared.open(targetUrl) }
      return ["ok": true, "message": "url_open_requested"]
    case "dump":
      ensureApp()
      return [
        "ok": true,
        "elements": dumpUi(
          filter: string(command.params["filter"]),
          maxResults: integer(command.params["maxResults"])
        ),
      ]
    case "tap-element":
      ensureApp()
      let ok = tapElement(filter: string(command.params["filter"]) ?? "")
      return ["ok": ok, "message": ok ? "tap_element" : "tap_element_failed"]
    case "tap":
      ensureApp()
      let ok = driver.tap(
        x: number(command.params["x"]) ?? 0,
        y: number(command.params["y"]) ?? 0
      )
      return ["ok": ok, "message": ok ? "tap" : "tap_failed"]
    case "swipe":
      ensureApp()
      let ok = driver.swipe(
        fromX: number(command.params["fromX"]) ?? 0,
        fromY: number(command.params["fromY"]) ?? 0,
        toX: number(command.params["toX"]) ?? 0,
        toY: number(command.params["toY"]) ?? 0,
        duration: (number(command.params["durationMs"]) ?? 100) / 1000
      )
      return ["ok": ok, "message": ok ? "swipe" : "swipe_failed"]
    case "text":
      ensureApp()
      focusFirstTextInput()
      let ok = typeText(string(command.params["text"]) ?? "")
      return ["ok": ok, "message": ok ? "text" : "text_failed"]
    case "screenshot":
      guard let data = driver.screenshot() else {
        return ["ok": false, "message": "screenshot_empty"]
      }
      return ["ok": true, "pngBase64": data.base64EncodedString()]
    case "stop":
      driver.terminate()
      return ["ok": true, "message": "stopped"]
    default:
      return ["ok": false, "message": "unknown_method"]
    }
  }

  private func ensureApp() {
    if currentBundleId == nil {
      driver.initialize(bundleIdentifier: "com.apple.springboard")
      currentBundleId = "com.apple.springboard"
    }
  }

  private func focusFirstTextInput() {
    guard let app = driver.app else { return }
    let field = app.textFields.firstMatch
    if field.exists && field.isHittable {
      field.tap()
    } else {
      let view = app.textViews.firstMatch
      if view.exists && view.isHittable {
        view.tap()
      }
    }
  }

  private func typeText(_ text: String) -> Bool {
    guard let app = driver.app else { return false }
    let submits = text.contains("\n") || text.contains("\r")
    let text = text.replacingOccurrences(of: "\r", with: "")
      .replacingOccurrences(of: "\n", with: "")
    if !text.isEmpty { driver.typeText(text) }
    return !submits || pressSubmitKey(in: app)
  }

  private func pressSubmitKey(in app: XCUIApplication) -> Bool {
    let labels = ["Go", "Search", "Return", "Done"]
    let predicate = NSPredicate(format: "label IN[c] %@", labels)
    let key = app.keyboards.buttons.matching(predicate).firstMatch
    guard key.exists && key.isHittable else { return false }
    key.tap()
    return true
  }

  private func dumpUi(filter: String?, maxResults: Int?) -> [[String: Any]] {
    guard let app = driver.app else { return [] }
    let normalizedFilter = filter.flatMap { nonEmpty($0) }?.lowercased()
    let limit = max(maxResults ?? 100, 0)
    guard limit > 0 else { return [] }

    let queries: [XCUIElementQuery] = [
      app.textFields,
      app.secureTextFields,
      app.textViews,
      app.searchFields,
      app.buttons,
      app.links,
      app.staticTexts,
    ]
    let predicate = normalizedFilter.map {
      NSPredicate(
        format: "identifier CONTAINS[c] %@ OR label CONTAINS[c] %@ OR value CONTAINS[c] %@",
        $0,
        $0,
        $0
      )
    }

    var elements: [[String: Any]] = []
    for baseQuery in queries {
      let query = predicate.map { baseQuery.matching($0) } ?? baseQuery
      var index = 0
      while elements.count < limit {
        let element = query.element(boundBy: index)
        index += 1
        guard element.exists else { break }
        elements.append(elementJson(element))
      }
      if elements.count >= limit { break }
    }
    return elements
  }

  private func tapElement(filter: String) -> Bool {
    guard let app = driver.app else { return false }
    let normalizedFilter = filter.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !normalizedFilter.isEmpty else { return false }
    let predicate = NSPredicate(
      format: "identifier CONTAINS[c] %@ OR label CONTAINS[c] %@ OR value CONTAINS[c] %@",
      normalizedFilter,
      normalizedFilter,
      normalizedFilter
    )
    let queries: [XCUIElementQuery] = [
      app.buttons,
      app.textFields,
      app.secureTextFields,
      app.textViews,
      app.searchFields,
      app.links,
      app.staticTexts,
    ]
    for query in queries {
      let element = query.matching(predicate).firstMatch
      guard element.exists && element.isEnabled else { continue }

      let frame = element.frame
      guard frame.width > 0, frame.height > 0 else { continue }

      // Safari exposes the ChatGPT send control as a tall web button. Its
      // reported hit-test state is unstable, and element.tap() can fail the
      // whole XCTest even after isHittable returned true. Use coordinate input
      // at the control's visible top edge for this semantic action.
      if normalizedFilter.localizedCaseInsensitiveContains("send") {
        let y = frame.height > frame.width * 3
          ? frame.minY + min(12, frame.height / 2)
          : frame.midY
        if driver.tap(x: frame.midX, y: y) {
          return true
        }
        continue
      }

      if element.isHittable {
        element.tap()
        return true
      }

      // Safari web-content buttons can be enabled and have a valid frame while
      // XCTest reports isHittable=false. Keep semantic tap-element actions
      // usable by falling back to the same frame-based coordinate injection
      // used by mobile_click_on_screen_at_coordinates.
      if driver.tap(x: frame.midX, y: frame.midY) {
        return true
      }
    }
    return false
  }

  private func elementJson(_ element: XCUIElement) -> [String: Any] {
    [
      "nodeId": "ios-\(UUID().uuidString)",
      "automationId": nonEmpty(element.identifier) as Any,
      "text": element.elementType == .staticText ? nonEmpty(element.label) as Any : NSNull(),
      "label": nonEmpty(element.label) as Any,
      "value": stringValue(element.value) as Any,
      "role": role(for: element.elementType),
      "bounds": [
        "x": element.frame.minX,
        "y": element.frame.minY,
        "width": element.frame.width,
        "height": element.frame.height,
      ],
      "enabled": element.isEnabled,
      "clickable": element.isHittable,
    ]
  }

  private func role(for type: XCUIElement.ElementType) -> String {
    switch type {
    case .button: return "button"
    case .staticText: return "text"
    case .textField, .secureTextField, .textView, .searchField: return "textField"
    case .link: return "link"
    case .image: return "image"
    case .scrollView, .table, .collectionView: return "scrollView"
    default: return "unknown"
    }
  }

  private func nonEmpty(_ value: String) -> String? {
    value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : value
  }

  private func stringValue(_ value: Any?) -> String? {
    guard let value, !(value is NSNull) else { return nil }
    return nonEmpty(String(describing: value))
  }

  private func nextCommand() -> Command? {
    for host in hosts {
      if let command = getCommand(url: "http://\(host):\(port)/next") { return command }
    }
    return nil
  }

  private func postResult(id: String, body: [String: Any]) {
    var payload = body
    payload["id"] = id
    for host in hosts {
      if post(url: "http://\(host):\(port)/result", json: payload) { return }
    }
  }

  private func getCommand(url: String) -> Command? {
    guard let data = request(url: url, method: "GET", body: nil),
          let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let id = object["id"] as? String,
          let method = object["method"] as? String else { return nil }
    return Command(id: id, method: method, params: object["params"] as? [String: Any] ?? [:])
  }

  private func post(url: String, json: [String: Any]) -> Bool {
    guard let body = try? JSONSerialization.data(withJSONObject: json) else { return false }
    return request(url: url, method: "POST", body: body) != nil
  }

  private func request(url: String, method: String, body: Data?) -> Data? {
    guard let requestUrl = URL(string: url) else { return nil }
    var request = URLRequest(url: requestUrl)
    request.httpMethod = method
    request.timeoutInterval = 20
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    if let token { request.setValue(token, forHTTPHeaderField: "X-MobileMcp-Token") }
    request.httpBody = body

    var result: Data?
    var failed = false
    let semaphore = DispatchSemaphore(value: 0)
    URLSession.shared.dataTask(with: request) { data, response, error in
      defer { semaphore.signal() }
      guard error == nil else { failed = true; return }
      let status = (response as? HTTPURLResponse)?.statusCode ?? 0
      if status == 204 { return }
      if (200..<300).contains(status) { result = data } else { failed = true }
    }.resume()
    _ = semaphore.wait(timeout: .now() + 22)
    return failed ? nil : result
  }

  private func string(_ value: Any?) -> String? {
    guard let value, !(value is NSNull) else { return nil }
    let text = String(describing: value).trimmingCharacters(in: .whitespacesAndNewlines)
    return text.isEmpty ? nil : text
  }

  private func number(_ value: Any?) -> CGFloat? {
    if let number = value as? NSNumber { return CGFloat(truncating: number) }
    if let text = string(value), let parsed = Double(text) { return CGFloat(parsed) }
    return nil
  }

  private func integer(_ value: Any?) -> Int? {
    if let number = value as? NSNumber { return number.intValue }
    if let text = string(value), let parsed = Int(text) { return parsed }
    return nil
  }
}

private struct Command {
  let id: String
  let method: String
  let params: [String: Any]
}

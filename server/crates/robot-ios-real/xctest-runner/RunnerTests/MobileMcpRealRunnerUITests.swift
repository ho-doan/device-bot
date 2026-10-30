import device_kit_lib_xctest
import Foundation
import Network
import UIKit
import XCTest

enum DumpError: Error, CustomStringConvertible {
  case unsupportedElementType(String)

  var description: String {
    switch self {
    case .unsupportedElementType(let elementType):
      return "unsupported elementType \(elementType)"
    }
  }
}

final class MobileMcpRealRunnerUITests: XCTestCase {
  /// Kept alive so the Bonjour browse still triggers Local Network privacy
  /// after `requestLocalNetworkPermission` returns into the command loop.
  private var localNetworkBrowser: NWBrowser?

  func testServe() {
    let environment = ProcessInfo.processInfo.environment
    let hosts = (environment["DEVICE_BOT_HOSTS"] ?? "")
      .split(separator: ",")
      .map(String.init)
      .filter { !$0.isEmpty }
    let port = Int(environment["DEVICE_BOT_PORT"] ?? "18787") ?? 18787
    let token = environment["DEVICE_BOT_TOKEN"]
    requestLocalNetworkPermission(host: hosts.first, port: port)
    MobileMcpRealHostAgent(hosts: hosts, port: port, token: token).run()
  }

  /// Ask iOS for Local Network access before the command loop. A first run
  /// shows the system sheet; a previous denial does not, so this opens
  /// Settings on the Local Network page and enables Device Bot there.
  private func requestLocalNetworkPermission(host: String?, port: Int) {
    guard let host, let url = bridgeUrl(host: host, port: port, path: "/next") else { return }
    triggerLocalNetworkAccess(url: url)
    if acceptLocalNetworkPrompt(timeout: 12) || !localNetworkProhibited(url) {
      print("Device Bot Local Network permission allowed")
      return
    }

    print("Device Bot opening Settings → Privacy & Security → Local Network")
    let settings = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
    let opened = openLocalNetworkSettings(settings)
    if opened {
      _ = enableDeviceBotLocalNetworkSwitch(in: settings)
    }

    let deadline = Date().addingTimeInterval(90)
    while Date() < deadline {
      if acceptLocalNetworkPrompt(timeout: 1) || !localNetworkProhibited(url) {
        print("Device Bot Local Network permission allowed")
        XCUIDevice.shared.press(.home)
        return
      }
      RunLoop.current.run(until: Date().addingTimeInterval(1))
    }
    if opened {
      print("Device Bot Local Network still blocked. Settings is open on Local Network; turn on Device Bot.")
    } else {
      print("Device Bot could not open Local Network settings. Turn on Device Bot under Settings → Privacy & Security → Local Network.")
    }
  }

  private func triggerLocalNetworkAccess(url: URL) {
    URLSession.shared.dataTask(with: url).resume()
    let browser = NWBrowser(for: .bonjour(type: "_http._tcp", domain: nil), using: .tcp)
    localNetworkBrowser = browser
    browser.start(queue: .main)
  }

  private func acceptLocalNetworkPrompt(timeout: TimeInterval) -> Bool {
    let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
    let allow = NSPredicate(format: "label == 'Allow' OR label == 'Cho phép'")
    let button = springboard.buttons.matching(allow).firstMatch
    let alertButton = springboard.alerts.buttons.matching(allow).firstMatch
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
      if button.exists && button.isHittable {
        button.tap()
        return true
      }
      if alertButton.exists && alertButton.isHittable {
        alertButton.tap()
        return true
      }
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
      let elementType = string(command.params["elementType"])
      do {
        let elements = try dumpUi(
          filter: string(command.params["filter"]),
          elementType: elementType,
          maxResults: integer(command.params["maxResults"])
        )
        var body: [String: Any] = ["ok": true, "elements": elements]
        if let elementType { body["appliedElementType"] = elementType }
        return body
      } catch let error as DumpError {
        return ["ok": false, "message": error.description]
      } catch {
        return ["ok": false, "message": "\(error)"]
      }
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
    case "ime-action":
      ensureApp()
      let action = string(command.params["action"]) ?? ""
      let known = ["next", "done", "go", "search", "send"].contains(action)
      guard known else {
        return ["ok": false, "message": "unsupported IME action \(action)"]
      }
      guard let app = driver.app else {
        return ["ok": false, "message": "IME action \(action) is not available on the keyboard"]
      }
      let ok = pressImeAction(action, in: app)
      return [
        "ok": ok,
        "message": ok ? "ime_action" : "IME action \(action) is not available on the keyboard",
      ]
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

  private func pressImeAction(_ action: String, in app: XCUIApplication) -> Bool {
    let label: String
    switch action {
    case "next": label = "Next"
    case "done": label = "Done"
    case "go": label = "Go"
    case "search": label = "Search"
    case "send": label = "Send"
    default: return false
    }
    let predicate = NSPredicate(format: "label ==[c] %@", label)
    let key = app.keyboards.buttons.matching(predicate).firstMatch
    guard key.exists, key.isHittable else { return false }
    key.tap()
    return true
  }

  private func dumpUi(filter: String?, elementType: String?, maxResults: Int?) throws -> [[String: Any]] {
    guard let app = driver.app else { return [] }
    let normalizedFilter = filter.flatMap { nonEmpty($0) }?.lowercased()
    let limit = max(maxResults ?? 100, 0)
    guard limit > 0 else { return [] }
    guard let queries = queries(for: elementType, in: app) else {
      throw DumpError.unsupportedElementType(elementType ?? "")
    }
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
      "focused": optionalFocus(element).map { $0 as Any } ?? NSNull(),
      "checked": checkedState(element).map { $0 as Any } ?? NSNull(),
      "selected": selectedState(element).map { $0 as Any } ?? NSNull(),
    ]
  }

  private func queries(for elementType: String?, in app: XCUIApplication) -> [XCUIElementQuery]? {
    switch elementType.flatMap({ nonEmpty($0) }) {
    case nil:
      return [
        app.textFields,
        app.secureTextFields,
        app.textViews,
        app.searchFields,
        app.buttons,
        app.links,
        app.staticTexts,
      ]
    case "textField":
      return [app.textFields, app.secureTextFields, app.textViews, app.searchFields]
    case "button":
      return [app.buttons]
    case "link":
      return [app.links]
    case "staticText":
      return [app.staticTexts]
    case "image":
      return [app.images]
    case "switch":
      return [app.switches]
    case "slider":
      return [app.sliders]
    case "checkBox":
      return [app.descendants(matching: .checkBox)]
    case "radio":
      return [app.descendants(matching: .radioButton)]
    default:
      return nil
    }
  }

  private func optionalFocus(_ element: XCUIElement) -> Bool? {
    let selector = NSSelectorFromString("hasFocus")
    guard element.responds(to: selector) else { return nil }
    return element.value(forKey: "hasFocus") as? Bool
  }

  private func checkedState(_ element: XCUIElement) -> Bool? {
    switch element.elementType {
    case .switch, .checkBox:
      if let value = stringValue(element.value)?.lowercased() {
        if value == "1" || value == "true" { return true }
        if value == "0" || value == "false" { return false }
      }
      return nil
    default:
      return nil
    }
  }

  private func selectedState(_ element: XCUIElement) -> Bool? {
    switch element.elementType {
    case .button, .switch, .checkBox, .radioButton:
      return element.isSelected
    default:
      return element.isSelected ? true : nil
    }
  }

  private func role(for type: XCUIElement.ElementType) -> String {
    switch type {
    case .button: return "button"
    case .staticText: return "text"
    case .textField, .secureTextField, .textView, .searchField: return "textField"
    case .link: return "link"
    case .image: return "image"
    case .checkBox: return "checkBox"
    case .switch: return "switch"
    case .radioButton: return "radio"
    case .slider: return "slider"
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
      if let command = getCommand(host: host) { return command }
    }
    return nil
  }

  private func postResult(id: String, body: [String: Any]) {
    var payload = body
    payload["id"] = id
    for host in hosts {
      if post(host: host, json: payload) { return }
    }
  }

  private func getCommand(host: String) -> Command? {
    guard let url = bridgeUrl(host: host, port: port, path: "/next") else { return nil }
    guard let data = request(url: url, method: "GET", body: nil),
          let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let id = object["id"] as? String,
          let method = object["method"] as? String else { return nil }
    return Command(id: id, method: method, params: object["params"] as? [String: Any] ?? [:])
  }

  private func post(host: String, json: [String: Any]) -> Bool {
    guard let url = bridgeUrl(host: host, port: port, path: "/result") else { return false }
    guard let body = try? JSONSerialization.data(withJSONObject: json) else { return false }
    return request(url: url, method: "POST", body: body) != nil
  }

  private func request(url: URL, method: String, body: Data?) -> Data? {
    var request = URLRequest(url: url)
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
      guard error == nil else {
        print("Device Bot bridge request failed (\(method) \(url.absoluteString)): \(error!.localizedDescription)")
        failed = true
        return
      }
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

private func bridgeUrl(host: String, port: Int, path: String) -> URL? {
  let host = host.trimmingCharacters(in: .whitespacesAndNewlines)
  guard !host.isEmpty, path.hasPrefix("/") else { return nil }
  let formattedHost: String
  if host.hasPrefix("[") && host.hasSuffix("]") {
    formattedHost = host
  } else if host.contains(":") {
    formattedHost = "[\(host)]"
  } else {
    formattedHost = host
  }
  return URL(string: "http://\(formattedHost):\(port)\(path)")
}

private struct Command {
  let id: String
  let method: String
  let params: [String: Any]
}

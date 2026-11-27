import device_kit_lib_xctest
import Foundation
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

func readUiSnapshot<T>(_ read: () throws -> T) rethrows -> T {
  do {
    return try read()
  } catch {
    let failure = error as NSError
    guard failure.domain == "com.apple.dt.xctest.ui-testing.error", failure.code == 10008 else {
      throw error
    }
    // The status changed between checking existence and taking a snapshot.
    // Discard that partial tree and read the current UI again, without input.
    return try read()
  }
}

/// XCTest-side command host for real devices.
///
/// The simulator runner has native filtered dumps and tap-element actions. The
/// real-device runner keeps the same command contract locally so the Rust
/// driver does not need to materialize the entire Safari tree for every tap.
final class MobileMcpRealHostAgent {
  private let driver = DeviceKitXCTestDriver()
  private let hosts: [String]
  private let port: Int
  private let token: String?
  private let deliverySessionId = UUID().uuidString
  private var lastResult: (id: String, body: [String: Any])?
  private var preferredHost: String?
  private var currentBundleId: String?
  private var running = true
  private var pollWasUnreachable = false

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
      // The bridge may resend the same ID after a lost HTTP response. Return
      // the original result without running the XCTest action a second time.
      if lastResult?.id != command.id {
        lastResult = (command.id, execute(command))
      }
      if let result = lastResult {
        postResult(id: result.id, body: result.body)
      }
      if command.method == "stop" { running = false }
    }
  }

  func execute(_ command: Command) -> [String: Any] {
    switch command.method {
    case "health":
      return ["ok": true, "bundleId": currentBundleId ?? "",
        "appForeground": currentBundleId.map { XCUIApplication(bundleIdentifier: $0).state == .runningForeground } ?? false,
        "deviceId": ProcessInfo.processInfo.environment["DEVICE_BOT_DEVICE_ID"] ?? ProcessInfo.processInfo.environment["SIMULATOR_UDID"] ?? "",
        "agentVersion": Bundle(for: MobileMcpRealRunnerUITests.self).object(forInfoDictionaryKey: "DeviceBotBuildId") as? String ?? "unknown"]
    case "launch":
      guard let bundleId = string(command.params["bundleId"]), !bundleId.isEmpty else {
        return ["ok": false, "message": "bundleId is required"]
      }
      driver.initialize(bundleIdentifier: bundleId)
      currentBundleId = bundleId
      driver.launch()
      return ["ok": true, "message": "app_launched"]
    case "terminate":
      guard let bundleId = string(command.params["bundleId"]), bundleId == currentBundleId,
            driver.app != nil else {
        return ["ok": false, "message": "Termination requires the selected target bundle"]
      }
      driver.terminate()
      return ["ok": true, "message": "app_terminated"]
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
        let elements = try readUiSnapshot {
          try dumpUi(
            filter: string(command.params["filter"]),
            elementType: elementType,
            maxResults: integer(command.params["maxResults"])
          )
        }
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
    case "long_press", "double_tap":
      ensureApp()
      guard let x = number(command.params["x"]), let y = number(command.params["y"]) else {
        return ["ok": false, "message": "gesture_coordinates_required"]
      }
      let ok = driver.pointerGesture(
        x: x, y: y, kind: command.method,
        durationMs: number(command.params["durationMs"]).map(Double.init)
      )
      return ["ok": ok, "message": ok ? command.method : "gesture_failed"]
    case "swipe":
      ensureApp()
      let ok = driver.swipe(
        fromX: number(command.params["fromX"]) ?? 0,
        fromY: number(command.params["fromY"]) ?? 0,
        toX: number(command.params["toX"]) ?? 0,
        toY: number(command.params["toY"]) ?? 0,
        duration: TimeInterval(number(command.params["durationMs"]) ?? 100) / 1000
      )
      return ["ok": ok, "message": ok ? "swipe" : "swipe_failed"]
    case "text":
      ensureApp()
      guard let text = command.params["text"] as? String else {
        return ["ok": false, "message": "text_required"]
      }
      // Preserve the focused input and the exact text payload, including spaces.
      let ok = typeText(text)
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
    case "screenInfo":
      let frame = driver.liveScreenBounds()
      return ["ok": true, "width": frame.width, "height": frame.height]
    case "screenshot":
      guard let data = driver.screenshot() else {
        return ["ok": false, "message": "screenshot_empty"]
      }
      return ["ok": true, "pngBase64": data.base64EncodedString()]
    case "stop":
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
    let predicate = keyboardActionPredicate(labels)
    let key = app.keyboards.buttons.matching(predicate).firstMatch
    guard key.exists && key.isHittable else { return false }
    key.tap()
    return true
  }

  private func keyboardActionPredicate(_ labels: [String]) -> NSPredicate {
    // Physical Vietnamese keyboards expose e.g. identifier "Next:" and label
    // "Tiếp". Match the requested semantic action without guessing translations.
    let identifiers = labels.flatMap { [$0, $0 + ":"] }
    return NSPredicate(format: "label IN[c] %@ OR identifier IN[c] %@", labels, identifiers)
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
    let keys = app.keyboards.buttons.matching(keyboardActionPredicate([label]))
    var availableKey: XCUIElement?
    let ready = XCTNSPredicateExpectation(
      predicate: NSPredicate { _, _ in
        availableKey = keys.allElementsBoundByIndex.first { $0.exists && $0.isHittable }
        return availableKey != nil
      }, object: nil
    )
    guard XCTWaiter.wait(for: [ready], timeout: 5) == .completed,
      let key = availableKey else { return false }
    key.tap()
    return true
  }

  private func dumpUi(filter: String?, elementType: String?, maxResults: Int?) throws -> [[String: Any]] {
    guard let app = driver.app else { return [] }
    let normalizedFilter = filter.flatMap { nonEmpty($0) }?.lowercased()
    let limit = max(maxResults ?? 100, 0)
    guard limit > 0 else { return [] }
    guard let typedQueries = queries(for: elementType, in: app) else {
      throw DumpError.unsupportedElementType(elementType ?? "")
    }
    let baseQueries = elementType == nil && normalizedFilter != nil
      ? [app.descendants(matching: .any)] : typedQueries
    let keyboard = app.keyboards.firstMatch
    let keyboardFrame = keyboard.exists ? keyboard.frame : .zero
    let predicate = normalizedFilter.map {
      NSPredicate(
        format: "identifier CONTAINS[c] %@ OR label CONTAINS[c] %@ OR value CONTAINS[c] %@",
        $0,
        $0,
        $0
      )
    }

    var elements: [[String: Any]] = []
    for baseQuery in baseQueries {
      let query = predicate.map { baseQuery.matching($0) } ?? baseQuery
      var index = 0
      while elements.count < limit {
        let element = query.element(boundBy: index)
        index += 1
        guard element.exists else { break }
        elements.append(try elementJson(element, keyboard: keyboard, keyboardFrame: keyboardFrame))
      }
      if elements.count >= limit { break }
    }
    return elements
  }

  private func tapElement(filter: String) -> Bool {
    guard let app = driver.app else { return false }
    let normalizedFilter = filter.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !normalizedFilter.isEmpty else { return false }
    let exact = NSPredicate(
      format: "identifier ==[c] %@ OR label ==[c] %@ OR value ==[c] %@",
      normalizedFilter, normalizedFilter, normalizedFilter
    )
    let contains = NSPredicate(
      format: "identifier CONTAINS[c] %@ OR label CONTAINS[c] %@ OR value CONTAINS[c] %@",
      normalizedFilter, normalizedFilter, normalizedFilter
    )
    let queries: [XCUIElementQuery] = [
      app.buttons, app.textFields, app.secureTextFields, app.textViews,
      app.searchFields, app.switches, app.descendants(matching: .checkBox),
      app.menuItems, app.links, app.staticTexts,
    ]
    // Prefer the actual Password field over the Show password button, while
    // preserving substring queries for existing browser flows.
    for predicate in [exact, contains] {
      for query in queries {
        let element = query.matching(predicate).firstMatch
        guard element.exists && element.isEnabled else { continue }
        let frame = element.frame
        guard frame.width > 0, frame.height > 0 else { continue }
        // Preserve the existing workaround for tall web send controls.
        if normalizedFilter.localizedCaseInsensitiveContains("send") {
          let y = frame.height > frame.width * 3
            ? frame.minY + min(12, frame.height / 2) : frame.midY
          if driver.tap(x: frame.midX, y: y) { return true }
          continue
        }
        if element.isHittable {
          element.tap()
          return true
        }
        if driver.tap(x: frame.midX, y: frame.midY) { return true }
      }
    }
    return false
  }

  private func elementJson(
    _ element: XCUIElement, keyboard: XCUIElement, keyboardFrame: CGRect
  ) throws -> [String: Any] {
    // Resolve native attributes once, as on the Simulator driver.
    let snapshot = try element.snapshot()
    let frame = snapshot.frame
    let coveredByKeyboard = keyboardFrame.contains(CGPoint(x: frame.midX, y: frame.midY))
    let keyboardMatch = coveredByKeyboard
      ? keyboard.descendants(matching: snapshot.elementType).matching(
        NSPredicate(format: "label == %@ AND identifier == %@", snapshot.label, snapshot.identifier)
      ).firstMatch : nil
    let isKeyboardControl = keyboardMatch.map { $0.exists && $0.frame == frame } ?? false
    return [
      "nodeId": "ios-\(UUID().uuidString)",
      "automationId": nonEmpty(snapshot.identifier) as Any,
      "text": snapshot.elementType == .staticText ? nonEmpty(snapshot.label) as Any : NSNull(),
      "label": nonEmpty(snapshot.label) as Any,
      "value": stringValue(snapshot.value) as Any,
      "role": role(for: snapshot.elementType),
      "bounds": [
        "x": frame.minX,
        "y": frame.minY,
        "width": frame.width,
        "height": frame.height,
      ],
      "enabled": snapshot.isEnabled,
      // Native Flutter hit-testing can include controls covered by the IME.
      "clickable": element.isHittable && (!coveredByKeyboard || isKeyboardControl),
      "focused": optionalFocus(snapshot as? NSObject).map { $0 as Any } ?? NSNull(),
      "checked": checkedState(snapshot).map { $0 as Any } ?? NSNull(),
      "selected": selectedState(snapshot).map { $0 as Any } ?? NSNull(),
    ]
  }

  private func queries(for elementType: String?, in app: XCUIApplication) -> [XCUIElementQuery]? {
    switch elementType.flatMap({ nonEmpty($0) }) {
    case nil:
      return [
        app.textFields, app.secureTextFields, app.textViews, app.searchFields,
        app.buttons, app.links, app.staticTexts, app.switches,
        app.descendants(matching: .checkBox), app.scrollViews,
        app.otherElements.matching(NSPredicate(format: "label != '' OR value != nil")),
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

  private func optionalFocus(_ element: NSObject?) -> Bool? {
    // UI focus and the text-input responder are distinct on iOS.
    let selector = NSSelectorFromString("hasKeyboardFocus")
    guard let element, element.responds(to: selector) else { return nil }
    return element.value(forKey: "hasKeyboardFocus") as? Bool
  }

  private func checkedState(_ element: XCUIElementAttributes) -> Bool? {
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

  private func selectedState(_ element: XCUIElementAttributes) -> Bool? {
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
    case .menuItem: return "menuItem"
    case .other: return "other"
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
    for host in orderedHosts() {
      // An empty 204 is a healthy poll, not a reason to try other hosts.
      guard let data = poll(host: host) else { continue }
      preferredHost = host
      guard !data.isEmpty,
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let id = object["id"] as? String,
            let method = object["method"] as? String else { return nil }
      return Command(id: id, method: method, params: object["params"] as? [String: Any] ?? [:])
    }
    return nil
  }

  private func orderedHosts() -> [String] {
    guard let preferredHost else { return hosts }
    return [preferredHost] + hosts.filter { $0 != preferredHost }
  }

  private func postResult(id: String, body: [String: Any]) {
    var payload = body
    payload["id"] = id
    for host in orderedHosts() {
      if post(host: host, json: payload) { return }
    }
  }

  private func poll(host: String) -> Data? {
    guard let base = bridgeUrl(host: host, port: port, path: "/next"),
          var components = URLComponents(url: base, resolvingAgainstBaseURL: false) else { return nil }
    components.queryItems = [
      URLQueryItem(name: "deviceId", value: ProcessInfo.processInfo.environment["DEVICE_BOT_DEVICE_ID"] ?? ""),
      URLQueryItem(name: "deliverySessionId", value: deliverySessionId)
    ]
    guard let url = components.url else { return nil }
    return request(url: url, method: "GET", body: nil)
  }

  private func post(host: String, json: [String: Any]) -> Bool {
    guard let url = bridgeUrl(host: host, port: port, path: "/result") else { return false }
    guard let body = try? JSONSerialization.data(withJSONObject: json) else { return false }
    return request(url: url, method: "POST", body: body) != nil
  }

  private func request(url: URL, method: String, body: Data?) -> Data? {
    var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData)
    request.httpMethod = method
    request.timeoutInterval = 20
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.setValue("close", forHTTPHeaderField: "Connection")
    if let token { request.setValue(token, forHTTPHeaderField: "X-MobileMcp-Token") }
    request.httpBody = body

    var result: Data?
    var failed = false
    var transportFailure = false
    var receivedHTTP = false
    let semaphore = DispatchSemaphore(value: 0)
    let task = URLSession.shared.dataTask(with: request) { data, response, error in
      defer { semaphore.signal() }
      receivedHTTP = response is HTTPURLResponse
      if let error {
        let endpoint = url.absoluteString.split(separator: "?", maxSplits: 1).first ?? ""
        let failure = error as NSError
        print("Device Bot bridge request failed (\(method) \(endpoint)): \(failure.domain) \(failure.code) \(failure.localizedDescription)")
        if let code = (error as? URLError)?.code {
          switch code {
          case .timedOut, .cannotFindHost, .cannotConnectToHost, .networkConnectionLost,
               .dnsLookupFailed, .notConnectedToInternet, .dataNotAllowed:
            transportFailure = !receivedHTTP
          default: break
          }
        }
        failed = true
        return
      }
      let status = (response as? HTTPURLResponse)?.statusCode ?? 0
      if (200..<300).contains(status) { result = data ?? Data() } else { failed = true }
    }
    task.resume()
    guard semaphore.wait(timeout: .now() + 22) == .success else {
      task.cancel()
      return nil
    }
    if method == "GET" && url.path == "/next" {
      if transportFailure {
        let endpoint = url.absoluteString.split(separator: "?", maxSplits: 1).first ?? ""
        print("Device Bot bridge unreachable (\(endpoint))")
        pollWasUnreachable = true
      } else if receivedHTTP && pollWasUnreachable {
        // Any HTTP response proves the network path, even if auth still fails.
        print("Device Bot bridge reachable")
        pollWasUnreachable = false
      }
    }
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

func bridgeUrl(host: String, port: Int, path: String) -> URL? {
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

struct Command {
  let id: String
  let method: String
  let params: [String: Any]
}

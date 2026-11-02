#if os(iOS)

import Foundation
import UIKit
import XCTest

/// Host-side XCTest agent. The UI-test process runs on the device and polls
/// the Mac JSON-RPC CLI for dump/tap/launch commands — the same split Mobile
/// MCP uses with WebDriverAgent, without calling `mobilecli`.
public final class DeviceKitXCTestHostAgent {
  public struct Command: Sendable {
    public let id: String
    public let method: String
    public let params: [String: Any]

    public init(id: String, method: String, params: [String: Any]) {
      self.id = id
      self.method = method
      self.params = params
    }
  }

  private let driver = DeviceKitXCTestDriver()
  private let hosts: [String]
  private let port: Int
  private let token: String?
  private var currentBundleId: String?
  public private(set) var running = true

  public init(hosts: [String], port: Int, token: String? = nil) {
    self.hosts = hosts.filter { !$0.isEmpty }
    self.port = port
    self.token = token?.isEmpty == false ? token : nil
  }

  public func run() {
    while running {
      guard let command = nextCommand() else {
        RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        continue
      }
      let result = execute(command)
      postResult(id: command.id, body: result)
      if command.method == "stop" {
        running = false
      }
    }
  }

  private func execute(_ command: Command) -> [String: Any] {
    switch command.method {
    case "health":
      return ["ok": true]
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
            let httpUrl = URL(string: rawUrl),
            ["http", "https"].contains(httpUrl.scheme?.lowercased()) else {
        return ["ok": false, "message": "invalid_http_url"]
      }
      let appId = string(command.params["appId"])
      let targetUrl: URL
      switch appId {
      case nil, "":
        targetUrl = httpUrl
      case "com.google.chrome.ios":
        guard var components = URLComponents(url: httpUrl, resolvingAgainstBaseURL: false) else {
          return ["ok": false, "message": "invalid_chrome_url"]
        }
        components.scheme = httpUrl.scheme?.lowercased() == "https" ? "googlechromes" : "googlechrome"
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
      DispatchQueue.main.async {
        UIApplication.shared.open(targetUrl)
      }
      return ["ok": true, "message": "url_open_requested"]
    case "dump":
      ensureApp()
      let snapshot = driver.dumpUi()
      return [
        "ok": true,
        "generation": snapshot.generation,
        "elements": snapshot.elements.map(elementJson),
      ]
    case "action":
      ensureApp()
      let nodeId = string(command.params["nodeId"]) ?? ""
      let generation = int(command.params["generation"]) ?? 0
      let actionName = string(command.params["action"]) ?? "press"
      let value = string(command.params["value"]) ?? ""
      let action: DeviceKitXCTestDriver.Action
      switch actionName {
      case "focus":
        action = .focus
      case "setValue":
        action = .setValue(value)
      case "scrollForward":
        action = .scrollForward
      case "scrollBackward":
        action = .scrollBackward
      default:
        action = .press
      }
      let ok = driver.performElementAction(
        nodeId: nodeId,
        snapshotGeneration: generation,
        action: action
      )
      return ["ok": ok, "message": ok ? "xctest" : "action_failed"]
    case "tap":
      ensureApp()
      let x = number(command.params["x"]) ?? 0
      let y = number(command.params["y"]) ?? 0
      let ok = driver.tap(x: x, y: y)
      return ["ok": ok, "message": ok ? "coordinate" : "tap_failed"]
    case "swipe":
      ensureApp()
      let ok = driver.swipe(
        fromX: number(command.params["fromX"]) ?? 0,
        fromY: number(command.params["fromY"]) ?? 0,
        toX: number(command.params["toX"]) ?? 0,
        toY: number(command.params["toY"]) ?? 0,
        duration: TimeInterval(number(command.params["durationMs"]) ?? 500) / 1000
      )
      return ["ok": ok, "message": ok ? "coordinate" : "swipe_failed"]
    case "text":
      ensureApp()
      guard let text = command.params["text"] as? String else {
        return ["ok": false, "message": "text_required"]
      }
      // Typing must preserve the caller's focused input and exact payload.
      let ok = driver.typeText(text)
      return ["ok": ok, "message": ok ? "text" : "type_failed"]
    case "button":
      let name = (string(command.params["name"]) ?? "").uppercased()
      if name == "HOME" {
        driver.pressHome()
        return ["ok": true, "message": "button"]
      }
      let ok = driver.pressBack()
      return ["ok": ok, "message": ok ? "button" : "button_failed"]
    case "screenshot":
      guard let data = driver.screenshot() else {
        return ["ok": false, "message": "screenshot_empty"]
      }
      return ["ok": true, "pngBase64": data.base64EncodedString()]
    case "clipboardGet":
      return ["ok": true, "text": driver.getClipboard() ?? ""]
    case "clipboardSet":
      driver.setClipboard(string(command.params["text"]) ?? "")
      return ["ok": true, "message": "clipboard"]
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

  private func nextCommand() -> Command? {
    for host in hosts {
      if let command = getCommand(url: "http://\(host):\(port)/next") {
        return command
      }
    }
    return nil
  }

  private func postResult(id: String, body: [String: Any]) {
    var payload = body
    payload["id"] = id
    for host in hosts {
      if post(url: "http://\(host):\(port)/result", json: payload) {
        return
      }
    }
  }

  private func getCommand(url: String) -> Command? {
    guard let data = request(url: url, method: "GET", body: nil) else {
      return nil
    }
    guard
      let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
      let id = object["id"] as? String,
      let method = object["method"] as? String
    else {
      return nil
    }
    return Command(
      id: id,
      method: method,
      params: object["params"] as? [String: Any] ?? [:]
    )
  }

  @discardableResult
  private func post(url: String, json: [String: Any]) -> Bool {
    guard let body = try? JSONSerialization.data(withJSONObject: json) else {
      return false
    }
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
    var failure = false
    let semaphore = DispatchSemaphore(value: 0)
    URLSession.shared.dataTask(with: request) { data, response, error in
      defer { semaphore.signal() }
      if error != nil {
        failure = true
        return
      }
      let status = (response as? HTTPURLResponse)?.statusCode ?? 0
      if status == 204 {
        result = Data()
        return
      }
      if (200..<300).contains(status) {
        result = data
      } else {
        failure = true
      }
    }.resume()
    _ = semaphore.wait(timeout: .now() + 22)
    if failure { return nil }
    if let result, result.isEmpty { return nil }
    return result
  }

  private func elementJson(_ element: DeviceKitXCTestDriver.UiElement) -> [String: Any] {
    [
      "nodeId": element.nodeId,
      "parentNodeId": element.parentNodeId as Any,
      "childNodeIds": element.childNodeIds,
      "automationId": element.automationId as Any,
      "text": element.text as Any,
      "label": element.label as Any,
      "value": element.value as Any,
      "role": element.role,
      "bounds": [
        "x": element.bounds.minX,
        "y": element.bounds.minY,
        "width": element.bounds.width,
        "height": element.bounds.height,
      ],
      "enabled": element.enabled,
      "clickable": element.clickable,
      "editable": element.editable,
      "focused": element.focused,
      "selected": element.selected,
      "checked": element.checked,
      "scrollable": element.scrollable,
    ]
  }

  private func string(_ value: Any?) -> String? {
    guard let value else { return nil }
    if value is NSNull { return nil }
    let text = String(describing: value).trimmingCharacters(in: .whitespacesAndNewlines)
    return text.isEmpty ? nil : text
  }

  private func number(_ value: Any?) -> CGFloat? {
    if let number = value as? NSNumber {
      return CGFloat(truncating: number)
    }
    if let text = string(value), let parsed = Double(text) {
      return CGFloat(parsed)
    }
    return nil
  }

  private func int(_ value: Any?) -> Int? {
    if let number = value as? NSNumber {
      return number.intValue
    }
    if let text = string(value) {
      return Int(text)
    }
    return nil
  }
}

#endif

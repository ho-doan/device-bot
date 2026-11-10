import Foundation
import UIKit
import XCTest

final class DeviceKitXCTestHostAgent {
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
      return ["ok": true, "bundleId": currentBundleId ?? "",
        "appForeground": currentBundleId.map { XCUIApplication(bundleIdentifier: $0).state == .runningForeground } ?? false,
        "deviceId": ProcessInfo.processInfo.environment["DEVICE_BOT_DEVICE_ID"] ?? ProcessInfo.processInfo.environment["SIMULATOR_UDID"] ?? "",
        "agentVersion": Bundle(for: MobileMcpSimulatorRunnerUITests.self).object(forInfoDictionaryKey: "DeviceBotBuildId") as? String ?? "unknown"]
    case "launch":
      guard let bundleId = string(command.params["bundleId"]), !bundleId.isEmpty else {
        return ["ok": false, "message": "bundleId is required"]
      }
      driver.initialize(bundleIdentifier: bundleId)
      currentBundleId = bundleId
      driver.launch()
      return ["ok": true, "message": "app_launched"]
    case "dump":
      ensureApp()
      let elementType = string(command.params["elementType"])
      do {
        let elements = try driver.dumpUi(
          filter: string(command.params["filter"]),
          elementType: elementType,
          maxResults: integer(command.params["maxResults"])
        ).map(elementJson)
        var body: [String: Any] = ["ok": true, "elements": elements]
        if let elementType { body["appliedElementType"] = elementType }
        return body
      } catch let error as DumpError {
        return ["ok": false, "message": error.description]
      } catch {
        return ["ok": false, "message": "\(error)"]
      }
    case "tap":
      ensureApp()
      let ok = driver.tap(x: number(command.params["x"]) ?? 0, y: number(command.params["y"]) ?? 0)
      return ["ok": ok, "message": ok ? "tap" : "tap_failed"]
    case "tap-element":
      ensureApp()
      let ok = driver.tapElement(filter: string(command.params["filter"]) ?? "")
      return ["ok": ok, "message": ok ? "tap_element" : "tap_element_failed"]
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
      guard let text = command.params["text"] as? String else {
        return ["ok": false, "message": "text_required"]
      }
      // Text is payload, not an identifier: preserve spaces and IME newlines.
      let ok = driver.typeText(text)
      return ["ok": ok, "message": ok ? "text" : "text_failed"]
    case "ime-action":
      ensureApp()
      let action = string(command.params["action"]) ?? ""
      let known = ["next", "done", "go", "search", "send"].contains(action)
      guard known else {
        return ["ok": false, "message": "unsupported IME action \(action)"]
      }
      let ok = driver.pressImeAction(action)
      return [
        "ok": ok,
        "message": ok ? "ime_action" : "IME action \(action) is not available on the keyboard",
      ]
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

  private func nextCommand() -> Command? {
    for host in hosts {
      if let command = getCommand(url: "http://\(host):\(port)/next?deviceId=\(ProcessInfo.processInfo.environment["DEVICE_BOT_DEVICE_ID"] ?? ProcessInfo.processInfo.environment["SIMULATOR_UDID"] ?? "")") { return command }
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

  private func elementJson(_ element: DeviceKitXCTestDriver.UiElement) -> [String: Any] {
    [
      "nodeId": element.nodeId,
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
      "focused": element.focused.map { $0 as Any } ?? NSNull(),
      "checked": element.checked.map { $0 as Any } ?? NSNull(),
      "selected": element.selected.map { $0 as Any } ?? NSNull(),
    ]
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

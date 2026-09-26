import XCTest

final class MobileMcpSimulatorRunnerUITests: XCTestCase {
  private let environment = ProcessInfo.processInfo.environment

  func testDispatch() throws {
    guard let resultPath = environment["MOBILE_MCP_RESULT_PATH"],
          let action = environment["MOBILE_MCP_ACTION"],
          let bundleID = environment["MOBILE_MCP_TARGET_APP_ID"]
    else {
      XCTFail("The runner request is incomplete.")
      return
    }

    do {
      let app = XCUIApplication(bundleIdentifier: bundleID)
      if app.state != .runningForeground {
        app.activate()
      }

      let result: [String: Any]
      switch action {
      case "tap":
        try coordinate(app).tap()
        result = ["ok": true]
      case "type":
        guard let text = environment["MOBILE_MCP_TEXT"], !text.isEmpty else {
          throw RunnerError.missing("text")
        }
        app.typeText(text)
        result = ["ok": true]
      case "swipe":
        let start = try coordinate(app)
        let end = try coordinate(app, xKey: "MOBILE_MCP_END_X", yKey: "MOBILE_MCP_END_Y")
        let duration = Double(environment["MOBILE_MCP_DURATION_SECONDS"] ?? "0.1") ?? 0.1
        start.press(forDuration: max(duration, 0.01), thenDragTo: end)
        result = ["ok": true]
      case "list-elements":
        result = ["ok": true, "elements": elements(
          from: app,
          matching: environment["MOBILE_MCP_ELEMENT_FILTER"],
          maxResults: Int(environment["MOBILE_MCP_MAX_RESULTS"] ?? "")
        )]
      default:
        throw RunnerError.unsupported(action)
      }
      try write(result, to: resultPath)
    } catch {
      try? write(["ok": false, "error": error.localizedDescription], to: resultPath)
      XCTFail(error.localizedDescription)
    }
  }

  private func coordinate(
    _ app: XCUIApplication,
    xKey: String = "MOBILE_MCP_X",
    yKey: String = "MOBILE_MCP_Y"
  ) throws -> XCUICoordinate {
    guard let x = Double(environment[xKey] ?? ""),
          let y = Double(environment[yKey] ?? "")
    else {
      throw RunnerError.invalidCoordinate
    }
    return app.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
      .withOffset(CGVector(dx: x, dy: y))
  }

  private func elements(
    from app: XCUIApplication,
    matching filter: String?,
    maxResults: Int?
  ) -> [[String: Any]] {
    let query: XCUIElementQuery
    if let filter, !filter.isEmpty {
      let predicate = NSPredicate(
        format: "identifier CONTAINS[c] %@ OR label CONTAINS[c] %@ OR value CONTAINS[c] %@",
        filter,
        filter,
        filter
      )
      query = app.descendants(matching: .any).matching(predicate)
    } else {
      query = app.descendants(matching: .any)
    }
    let count = min(query.count, max(maxResults ?? query.count, 1))
    return (0..<count).map { index in
      let element = query.element(boundBy: index)
      let frame = element.frame
      return [
        "className": String(describing: element.elementType),
        "resourceId": element.identifier,
        "text": element.value.map { String(describing: $0) } ?? "",
        "contentDescription": element.label,
        "left": Int(frame.minX.rounded()),
        "top": Int(frame.minY.rounded()),
        "right": Int(frame.maxX.rounded()),
        "bottom": Int(frame.maxY.rounded()),
        "clickable": element.isHittable,
        "enabled": element.isEnabled,
      ]
    }
  }

  private func write(_ result: [String: Any], to path: String) throws {
    let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
    try data.write(to: URL(fileURLWithPath: path), options: .atomic)
  }
}

private enum RunnerError: LocalizedError {
  case missing(String)
  case invalidCoordinate
  case unsupported(String)

  var errorDescription: String? {
    switch self {
    case .missing(let name): "Missing runner value: \(name)."
    case .invalidCoordinate: "Runner coordinates must be numeric."
    case .unsupported(let action): "Unsupported runner action: \(action)."
    }
  }
}

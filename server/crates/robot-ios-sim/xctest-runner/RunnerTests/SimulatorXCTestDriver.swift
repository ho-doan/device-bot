import CoreGraphics
import Foundation
import XCTest

final class DeviceKitXCTestDriver {
  struct UiElement {
    let nodeId: String
    let automationId: String?
    let text: String?
    let label: String?
    let value: String?
    let role: String
    let bounds: CGRect
    let enabled: Bool
    let clickable: Bool
  }

  private(set) var app: XCUIApplication?
  private var generation = 0

  func initialize(bundleIdentifier: String) {
    app = XCUIApplication(bundleIdentifier: bundleIdentifier)
    generation = 0
  }

  func launch() {
    app?.launch()
  }

  func dumpUi(filter: String? = nil, maxResults: Int? = nil) -> [UiElement] {
    guard let app else { return [] }
    generation += 1
    let normalizedFilter = filter.flatMap { nonEmpty($0) }?.lowercased()
    let limit = maxResults.map { max($0, 0) } ?? 100
    guard limit > 0 else { return [] }
    var elements: [UiElement] = []
    let baseQueries: [XCUIElementQuery] = [
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
    for baseQuery in baseQueries {
      let query = predicate.map { baseQuery.matching($0) } ?? baseQuery
      var index = 0
      while elements.count < limit {
        let element = query.element(boundBy: index)
        index += 1
        guard element.exists else { break }
        let frame = element.frame
        let label = nonEmpty(element.label)
        let value = stringValue(element.value)
        elements.append(
          UiElement(
            nodeId: "ios-\(generation)-\(elements.count)",
            automationId: nonEmpty(element.identifier),
            text: element.elementType == XCUIElement.ElementType.staticText ? label : nil,
            label: label,
            value: value,
            role: role(for: element.elementType),
            bounds: frame,
            enabled: element.isEnabled,
            clickable: element.isHittable
          )
        )
      }
      if elements.count >= limit { break }
    }
    return elements
  }

  func tap(x: CGFloat, y: CGFloat) -> Bool {
    guard let app else { return false }
    let frame = app.frame
    guard frame.width > 0, frame.height > 0, frame.contains(CGPoint(x: x, y: y)) else {
      return false
    }
    app.coordinate(withNormalizedOffset: .zero)
      .withOffset(CGVector(dx: x, dy: y))
      .tap()
    return true
  }

  func swipe(fromX: CGFloat, fromY: CGFloat, toX: CGFloat, toY: CGFloat, duration: TimeInterval) -> Bool {
    guard let app else { return false }
    let frame = app.frame
    let startPoint = CGPoint(x: fromX, y: fromY)
    let endPoint = CGPoint(x: toX, y: toY)
    guard frame.width > 0, frame.height > 0, frame.contains(startPoint), frame.contains(endPoint) else {
      return false
    }
    let start = app.coordinate(withNormalizedOffset: .zero)
      .withOffset(CGVector(dx: fromX, dy: fromY))
    let end = app.coordinate(withNormalizedOffset: .zero)
      .withOffset(CGVector(dx: toX, dy: toY))
    start.press(forDuration: max(duration, 0.01), thenDragTo: end)
    return true
  }

  func typeText(_ text: String) -> Bool {
    guard let app else { return false }
    let submits = text.contains("\n") || text.contains("\r")
    let text = text.replacingOccurrences(of: "\r", with: "")
      .replacingOccurrences(of: "\n", with: "")
    if !text.isEmpty { app.typeText(text) }
    return !submits || pressSubmitKey(in: app)
  }

  func terminate() {
    app?.terminate()
    app = nil
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

  private func pressSubmitKey(in app: XCUIApplication) -> Bool {
    let labels = ["Go", "Search", "Return", "Done"]
    let predicate = NSPredicate(format: "label IN[c] %@", labels)
    let key = app.keyboards.buttons.matching(predicate).firstMatch
    guard key.exists, key.isHittable else { return false }
    key.tap()
    return true
  }
}

import CoreGraphics
import Foundation
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
    let focused: Bool?
    let checked: Bool?
    let selected: Bool?
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

  func dumpUi(filter: String? = nil, elementType: String? = nil, maxResults: Int? = nil) throws -> [UiElement] {
    guard let app else { return [] }
    generation += 1
    let normalizedFilter = filter.flatMap { nonEmpty($0) }?.lowercased()
    let limit = maxResults.map { max($0, 0) } ?? 100
    guard limit > 0 else { return [] }
    guard let typedQueries = queries(for: elementType, in: app) else {
      throw DumpError.unsupportedElementType(elementType ?? "")
    }
    let baseQueries = elementType == nil && normalizedFilter != nil
      ? [app.descendants(matching: .any)] : typedQueries
    var elements: [UiElement] = []
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
    for baseQuery in baseQueries {
      let query = predicate.map { baseQuery.matching($0) } ?? baseQuery
      var index = 0
      while elements.count < limit {
        let element = query.element(boundBy: index)
        index += 1
        guard element.exists else { break }
        // Read attributes from one snapshot; resolving each property separately
        // becomes expensive on trees that also contain the system keyboard.
        let snapshot = try element.snapshot()
        let frame = snapshot.frame
        let label = nonEmpty(snapshot.label)
        let value = stringValue(snapshot.value)
        let coveredByKeyboard = keyboardFrame.contains(CGPoint(x: frame.midX, y: frame.midY))
        let keyboardMatch = coveredByKeyboard
          ? keyboard.descendants(matching: snapshot.elementType).matching(
            NSPredicate(format: "label == %@ AND identifier == %@", snapshot.label, snapshot.identifier)
          ).firstMatch : nil
        let isKeyboardControl = keyboardMatch.map { $0.exists && $0.frame == frame } ?? false
        elements.append(
          UiElement(
            nodeId: "ios-\(generation)-\(elements.count)",
            automationId: nonEmpty(snapshot.identifier),
            text: snapshot.elementType == XCUIElement.ElementType.staticText ? label : nil,
            label: label,
            value: value,
            role: role(for: snapshot.elementType),
            bounds: frame,
            enabled: snapshot.isEnabled,
            // Flutter can report a control behind the IME as hittable. Clients
            // tap the reported bounds, so reject a center covered by the IME.
            clickable: element.isHittable && (!coveredByKeyboard || isKeyboardControl),
            focused: optionalFocus(snapshot as? NSObject),
            checked: checkedState(snapshot),
            selected: selectedState(snapshot)
          )
        )
      }
      if elements.count >= limit { break }
    }
    return elements
  }

  func pressImeAction(_ action: String) -> Bool {
    guard let app else { return false }
    let label: String
    switch action {
    case "next": label = "Next"
    case "done": label = "Done"
    case "go": label = "Go"
    case "search": label = "Search"
    case "send": label = "Send"
    default: return false
    }
    let keys = app.keyboards.buttons.matching(NSPredicate(format: "label ==[c] %@", label))
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

  func tapElement(filter: String) -> Bool {
    guard let app else { return false }
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
          if tap(x: frame.midX, y: y) { return true }
          continue
        }
        if element.isHittable {
          element.tap()
          return true
        }
        if tap(x: frame.midX, y: frame.midY) { return true }
      }
    }
    return false
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

  private func pressSubmitKey(in app: XCUIApplication) -> Bool {
    let labels = ["Go", "Search", "Return", "Done"]
    let predicate = NSPredicate(format: "label IN[c] %@", labels)
    let key = app.keyboards.buttons.matching(predicate).firstMatch
    guard key.exists, key.isHittable else { return false }
    key.tap()
    return true
  }
}

#if os(iOS)

import UIKit
import XCTest

/// Low-level iOS automation driver for an XCTest/XCUITest runner.
///
/// This target is test-only. It is exported as a separate Swift product for
/// the server's native UI-test runner, so native UI-test targets can reuse the
/// driver without linking XCTest into the production server binary.
public final class DeviceKitXCTestDriver {
  public struct UiElement {
    public let nodeId: String
    public let parentNodeId: String?
    public internal(set) var childNodeIds: [String]
    public let automationId: String?
    public let text: String?
    public let label: String?
    public let value: String?
    public let role: String
    public let bounds: CGRect
    public let enabled: Bool
    public let clickable: Bool
    public let editable: Bool
    public let focused: Bool
    public let selected: Bool
    public let checked: Bool
    public let scrollable: Bool

    fileprivate init(
      nodeId: String,
      parentNodeId: String?,
      childNodeIds: [String],
      automationId: String?,
      text: String?,
      label: String?,
      value: String?,
      role: String,
      bounds: CGRect,
      enabled: Bool,
      clickable: Bool,
      editable: Bool,
      focused: Bool,
      selected: Bool,
      checked: Bool,
      scrollable: Bool
    ) {
      self.nodeId = nodeId
      self.parentNodeId = parentNodeId
      self.childNodeIds = childNodeIds
      self.automationId = automationId
      self.text = text
      self.label = label
      self.value = value
      self.role = role
      self.bounds = bounds
      self.enabled = enabled
      self.clickable = clickable
      self.editable = editable
      self.focused = focused
      self.selected = selected
      self.checked = checked
      self.scrollable = scrollable
    }
  }

  public struct UiSnapshot {
    public let generation: Int
    public let elements: [UiElement]
  }

  public enum Action {
    case press
    case focus
    case setValue(String)
    case scrollForward
    case scrollBackward
  }

  public private(set) var app: XCUIApplication?
  private var generation = 0
  private var elementsByNodeId: [String: XCUIElement] = [:]

  public init() {}

  public func initialize(bundleIdentifier: String) {
    app = XCUIApplication(bundleIdentifier: bundleIdentifier)
    generation = 0
    elementsByNodeId.removeAll()
  }

  public func launch() {
    precondition(app != nil, "Call initialize(bundleIdentifier:) first.")
    app?.launch()
  }

  public func terminate() {
    app?.terminate()
    generation += 1
    elementsByNodeId.removeAll()
  }

  public func dumpUi() -> UiSnapshot {
    guard let app else {
      preconditionFailure("Call initialize(bundleIdentifier:) first.")
    }

    generation += 1
    elementsByNodeId.removeAll()
    var elements: [UiElement] = []
    // ChatGPT-scale trees time out if every node re-queries `.any` children.
    // Collect the interactive controls the host actually taps and types into.
    let queries = [
      app.textFields,
      app.secureTextFields,
      app.textViews,
      app.searchFields,
      app.buttons,
    ]
    for query in queries {
      for child in query.allElementsBoundByIndex {
        appendSnapshotElement(child, parentIndex: nil, to: &elements, recurse: false)
      }
    }

    return UiSnapshot(generation: generation, elements: elements)
  }

  @discardableResult
  public func performElementAction(
    nodeId: String,
    snapshotGeneration: Int,
    action: Action
  ) -> Bool {
    guard snapshotGeneration == generation,
          let element = elementsByNodeId[nodeId],
          element.exists,
          element.isEnabled else {
      return false
    }

    switch action {
    case .press:
      guard element.isHittable else { return false }
      element.tap()
    case .focus:
      guard element.isHittable else { return false }
      element.tap()
    case let .setValue(value):
      guard element.isHittable else { return false }
      element.tap()
      element.typeText(value)
    case .scrollForward:
      element.swipeUp()
    case .scrollBackward:
      element.swipeDown()
    }
    return true
  }

  @discardableResult
  public func tap(x: CGFloat, y: CGFloat) -> Bool {
    guard let app else { return false }
    let frame = app.frame
    guard frame.width > 0,
          frame.height > 0,
          frame.contains(CGPoint(x: x, y: y)) else {
      return false
    }
    let coordinate = app.coordinate(
      withNormalizedOffset: CGVector(
        dx: (x - frame.minX) / frame.width,
        dy: (y - frame.minY) / frame.height
      )
    )
    coordinate.tap()
    return true
  }

  @discardableResult
  public func swipe(
    fromX: CGFloat,
    fromY: CGFloat,
    toX: CGFloat,
    toY: CGFloat,
    duration: TimeInterval
  ) -> Bool {
    guard let app else { return false }
    let frame = app.frame
    guard frame.width > 0,
          frame.height > 0,
          frame.contains(CGPoint(x: fromX, y: fromY)),
          frame.contains(CGPoint(x: toX, y: toY)) else {
      return false
    }
    let start = app.coordinate(
      withNormalizedOffset: CGVector(
        dx: (fromX - frame.minX) / frame.width,
        dy: (fromY - frame.minY) / frame.height
      )
    )
    let end = app.coordinate(
      withNormalizedOffset: CGVector(
        dx: (toX - frame.minX) / frame.width,
        dy: (toY - frame.minY) / frame.height
      )
    )
    start.press(forDuration: max(duration, 0.01), thenDragTo: end)
    return true
  }

  @discardableResult
  public func typeText(_ text: String) -> Bool {
    guard let app else { return false }
    app.typeText(text)
    return true
  }

  @discardableResult
  public func pressBack() -> Bool {
    guard let app else { return false }
    let backButton = app.navigationBars.buttons.firstMatch
    guard backButton.exists else { return false }
    backButton.tap()
    return true
  }

  public func pressHome() {
    XCUIDevice.shared.press(.home)
  }

  public func screenshot() -> Data? {
    XCUIScreen.main.screenshot().pngRepresentation
  }

  public func getClipboard() -> String? {
    UIPasteboard.general.string
  }

  public func setClipboard(_ text: String) {
    UIPasteboard.general.string = text
  }

  private func stringValue(_ value: Any?) -> String? {
    guard let value else { return nil }
    if value is NSNull { return nil }
    if let string = value as? String, !string.isEmpty {
      return string
    }
    let description = String(describing: value)
    return description.isEmpty ? nil : description
  }

  private func role(for type: XCUIElement.ElementType) -> String {
    switch type {
    case .button:
      return "button"
    case .staticText:
      return "text"
    case .textField, .secureTextField, .textView, .searchField:
      return "textField"
    case .image:
      return "image"
    case .checkBox:
      return "checkbox"
    case .radioButton:
      return "radio"
    case .switch:
      return "switch"
    case .slider:
      return "slider"
    case .table, .collectionView:
      return "list"
    case .cell:
      return "listItem"
    case .link:
      return "link"
    case .scrollView:
      return "scrollView"
    default:
      return "unknown"
    }
  }

  private func appendSnapshotElement(
    _ element: XCUIElement,
    parentIndex: Int?,
    to elements: inout [UiElement],
    recurse: Bool = true
  ) {
    guard element.exists else { return }

    let nodeId = "ios-\(generation)-\(elements.count)"
    elementsByNodeId[nodeId] = element
    let index = elements.count
    let identifier = element.identifier.isEmpty ? nil : element.identifier
    let label = element.label.isEmpty ? nil : element.label
    let value = stringValue(element.value)
    let isEditable = element.elementType == .textField
      || element.elementType == .secureTextField
      || element.elementType == .textView
      || element.elementType == .searchField
    let isScrollable = element.elementType == .scrollView
      || element.elementType == .table
      || element.elementType == .collectionView

    elements.append(
      UiElement(
        nodeId: nodeId,
        parentNodeId: parentIndex.map { elements[$0].nodeId },
        childNodeIds: [],
        automationId: identifier,
        text: element.elementType == .staticText ? label : nil,
        label: label,
        value: value,
        role: role(for: element.elementType),
        bounds: element.frame,
        enabled: element.isEnabled,
        clickable: element.isHittable,
        editable: isEditable,
        focused: false,
        selected: false,
        checked: false,
        scrollable: isScrollable
      )
    )

    if let parentIndex {
      elements[parentIndex].childNodeIds.append(nodeId)
    }

    guard recurse else { return }
    for child in element.children(matching: .any).allElementsBoundByIndex {
      appendSnapshotElement(child, parentIndex: index, to: &elements)
    }
  }
}

#endif

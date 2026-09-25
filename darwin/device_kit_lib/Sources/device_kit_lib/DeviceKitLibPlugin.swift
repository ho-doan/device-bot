#if os(iOS)

import Flutter
import UIKit

public final class DeviceKitLibPlugin: NSObject, FlutterPlugin {
  private let runtimeDriver = IosRuntimeDriver()

  public static func register(with registrar: FlutterPluginRegistrar) {
    let instance = DeviceKitLibPlugin()
    DeviceKitHostApiSetup.setUp(
      binaryMessenger: registrar.messenger(),
      api: instance
    )
  }
}

extension DeviceKitLibPlugin: DeviceKitHostApi {
  func initialize(config: DriverConfig) throws {
    runtimeDriver.initialize(config: config)
  }

  func dispose() throws {
    runtimeDriver.dispose()
  }

  func getDeviceInfo() throws -> DeviceInfo {
    runtimeDriver.getDeviceInfo()
  }

  func openAccessibilitySettings() throws -> ActionResult {
    runtimeDriver.openAccessibilitySettings()
  }

  func launchApp(packageName: String) throws -> ActionResult {
    runtimeDriver.launchApp(packageName: packageName)
  }

  func dumpUi() throws -> UiSnapshot {
    runtimeDriver.dumpUi()
  }

  func performElementAction(
    nodeId: String,
    generation: Int64,
    action: UiAction,
    value: String?
  ) throws -> ActionResult {
    runtimeDriver.performElementAction(
      nodeId: nodeId,
      generation: generation,
      action: action,
      value: value
    )
  }

  func tap(x: Double, y: Double) throws -> ActionResult {
    runtimeDriver.tap(x: x, y: y)
  }

  func swipe(
    fromX: Double,
    fromY: Double,
    toX: Double,
    toY: Double,
    durationMs: Int64
  ) throws -> ActionResult {
    runtimeDriver.swipe(
      fromX: fromX,
      fromY: fromY,
      toX: toX,
      toY: toY,
      durationMs: durationMs
    )
  }

  func typeText(text: String) throws -> ActionResult {
    runtimeDriver.typeText(text: text)
  }

  func pressBack() throws -> ActionResult {
    runtimeDriver.pressBack()
  }

  func pressHome() throws -> ActionResult {
    runtimeDriver.pressHome()
  }

  func screenshot() throws -> FlutterStandardTypedData {
    runtimeDriver.screenshot()
  }

  func requestScreenCapture() throws -> ActionResult {
    runtimeDriver.requestScreenCapture()
  }

  func getClipboard() throws -> String? {
    runtimeDriver.getClipboard()
  }

  func setClipboard(text: String) throws -> ActionResult {
    runtimeDriver.setClipboard(text: text)
  }
}

private final class IosRuntimeDriver {
  private var generation: Int64 = 0
  private var objectsByNodeId: [String: AnyObject] = [:]

  func initialize(config: DriverConfig) {
    generation = 0
    objectsByNodeId.removeAll()
    if config.enableLogs {
      NSLog("DeviceKit iOS session started: %@", config.sessionId)
    }
  }

  func dispose() {
    objectsByNodeId.removeAll()
  }

  func getDeviceInfo() -> DeviceInfo {
    let device = UIDevice.current
    return DeviceInfo(
      platform: "iOS",
      osVersion: device.systemVersion,
      model: device.model,
      deviceName: device.name,
      physicalDevice: !ProcessInfo.processInfo.environment.keys.contains("SIMULATOR_DEVICE_NAME")
    )
  }

  func openAccessibilitySettings() -> ActionResult {
    guard let url = URL(string: UIApplication.openSettingsURLString) else {
      return failure("Unable to create the iOS Settings URL.")
    }
    UIApplication.shared.open(url)
    return success(changed: false)
  }

  func launchApp(packageName: String) -> ActionResult {
    guard packageName.contains("://"), let url = URL(string: packageName) else {
      return failure(
        "iOS cannot launch an arbitrary bundle identifier from a plugin runtime. "
          + "Use XCTest/XCUITest with the bundle identifier or provide a URL scheme."
      )
    }
    UIApplication.shared.open(url)
    return success(changed: false)
  }

  func dumpUi() -> UiSnapshot {
    generation += 1
    objectsByNodeId.removeAll()
    var nodes: [UiNode] = []
    let windows = UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .flatMap(\.windows)
      .filter { !$0.isHidden && $0.alpha > 0 }

    for window in windows {
      appendAccessibleObjects(in: window, parentNodeId: nil, to: &nodes)
    }

    return UiSnapshot(generation: generation, nodes: nodes)
  }

  func performElementAction(
    nodeId: String,
    generation requestedGeneration: Int64,
    action: UiAction,
    value: String?
  ) -> ActionResult {
    guard requestedGeneration == generation else {
      return failure("The UI snapshot is stale; dump the UI again before acting.")
    }
    guard objectsByNodeId[nodeId] != nil else {
      return failure("The UI node is no longer available.")
    }

    // Cross-app semantic actions are implemented by the XCTest/XCUITest
    // driver in the UI-test target. UIKit has no public API for injecting a
    // tap into another application from a plugin process.
    _ = action
    _ = value
    return failure(
      "Cross-app element actions require XCTest/XCUITest; "
        + "the plugin runtime cannot inject them."
    )
  }

  func tap(x: Double, y: Double) -> ActionResult {
    _ = x
    _ = y
    return failure(
      "Cross-app coordinate actions require XCTest/XCUITest; "
        + "the plugin runtime cannot inject touch events."
    )
  }

  func swipe(
    fromX: Double,
    fromY: Double,
    toX: Double,
    toY: Double,
    durationMs: Int64
  ) -> ActionResult {
    _ = (fromX, fromY, toX, toY, durationMs)
    return failure(
      "Cross-app swipes require XCTest/XCUITest; "
        + "the plugin runtime cannot inject touch events."
    )
  }

  func typeText(text: String) -> ActionResult {
    _ = text
    return failure(
      "Cross-app text input requires XCTest/XCUITest; "
        + "the plugin runtime cannot inject keyboard events."
    )
  }

  func pressBack() -> ActionResult {
    guard let navigationController = foregroundViewController()?.navigationController,
          navigationController.viewControllers.count > 1 else {
      return failure("No navigable view controller is available.")
    }
    navigationController.popViewController(animated: true)
    return success(changed: true)
  }

  func pressHome() -> ActionResult {
    failure("iOS does not expose a public runtime API for pressing the Home button.")
  }

  func screenshot() -> FlutterStandardTypedData {
    let window = foregroundWindow()
    let renderer = UIGraphicsImageRenderer(bounds: window?.bounds ?? .zero)
    let image = renderer.image { context in
      if let window {
        window.layer.render(in: context.cgContext)
      }
    }
    return FlutterStandardTypedData(bytes: image.pngData() ?? Data())
  }

  func requestScreenCapture() -> ActionResult {
    // A plugin can capture its own foreground window without MediaProjection
    // or a user consent dialog. XCTest handles full-device screenshots.
    success(changed: false)
  }

  func getClipboard() -> String? {
    UIPasteboard.general.string
  }

  func setClipboard(text: String) -> ActionResult {
    UIPasteboard.general.string = text
    return success(changed: false)
  }

  private func appendAccessibleObjects(
    in view: UIView,
    parentNodeId: String?,
    to nodes: inout [UiNode]
  ) {
    if let accessibilityElements = view.accessibilityElements {
      for element in accessibilityElements {
        if let object = element as AnyObject? {
          append(object, parentNodeId: parentNodeId, to: &nodes)
        }
      }
      return
    }

    if view.isAccessibilityElement {
      append(view, parentNodeId: parentNodeId, to: &nodes)
      return
    }

    for child in view.subviews {
      appendAccessibleObjects(in: child, parentNodeId: parentNodeId, to: &nodes)
    }
  }

  private func append(
    _ object: AnyObject,
    parentNodeId: String?,
    to nodes: inout [UiNode]
  ) {
    let nodeId = "ios-\(generation)-\(nodes.count)"
    if objectsByNodeId[nodeId] != nil {
      return
    }
    objectsByNodeId[nodeId] = object

    let view = object as? UIView
    let accessibilityElement = object as? UIAccessibilityElement
    let identifier = (object as? UIAccessibilityIdentification)?.accessibilityIdentifier
    let label = accessibilityElement?.accessibilityLabel ?? view?.accessibilityLabel
    let value = accessibilityElement?.accessibilityValue ?? view?.accessibilityValue
    let frame = accessibilityElement?.accessibilityFrame ?? view?.accessibilityFrame ?? .zero
    let traits = accessibilityElement?.accessibilityTraits ?? view?.accessibilityTraits ?? []
    let role = role(for: traits, view: view)
    let text = (view as? UILabel)?.text
    let enabled = view.map { view in
      guard !view.isHidden, view.alpha > 0 else { return false }
      if let control = view as? UIControl {
        return control.isEnabled
      }
      return true
    } ?? true
    let editable = view is UITextField || view is UITextView
    let clickable = traits.contains(.button) || view is UIControl

    nodes.append(UiNode(
      nodeId: nodeId,
      parentNodeId: parentNodeId,
      childNodeIds: [],
      automationId: identifier,
      text: text,
      label: label,
      value: value,
      role: role,
      bounds: RectData(
        x: frame.origin.x,
        y: frame.origin.y,
        width: frame.size.width,
        height: frame.size.height
      ),
      enabled: enabled,
      clickable: clickable,
      editable: editable,
      focused: view?.isFirstResponder ?? false,
      selected: false,
      checked: false,
      scrollable: view is UIScrollView
    ))

    if let parentNodeId,
       let parentIndex = nodes.firstIndex(where: { $0.nodeId == parentNodeId }) {
      nodes[parentIndex].childNodeIds.append(nodeId)
    }
  }

  private func role(for traits: UIAccessibilityTraits, view: UIView?) -> String {
    if traits.contains(.button) { return "button" }
    if traits.contains(.link) { return "link" }
    if traits.contains(.image) { return "image" }
    if traits.contains(.header) { return "text" }
    if view is UITextField || view is UITextView { return "textField" }
    if view is UIScrollView { return "scrollView" }
    if view is UILabel { return "text" }
    return "unknown"
  }

  private func foregroundWindow() -> UIWindow? {
    UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .flatMap(\.windows)
      .first { $0.isKeyWindow } ?? UIApplication.shared.windows.first
  }

  private func foregroundViewController() -> UIViewController? {
    var controller = foregroundWindow()?.rootViewController
    while let presented = controller?.presentedViewController {
      controller = presented
    }
    return controller
  }

  private func success(changed: Bool) -> ActionResult {
    ActionResult(success: true, message: nil, uiChanged: changed)
  }

private func failure(_ message: String) -> ActionResult {
    ActionResult(success: false, message: message, uiChanged: false)
  }
}

#endif

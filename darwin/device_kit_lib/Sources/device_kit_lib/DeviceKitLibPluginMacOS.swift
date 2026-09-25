#if os(macOS)

import ApplicationServices
import Cocoa
import FlutterMacOS

public final class DeviceKitLibPlugin: NSObject, FlutterPlugin {
  private let runtimeDriver = MacOSAccessibilityDriver()

  public static func register(with registrar: FlutterPluginRegistrar) {
    let instance = DeviceKitLibPlugin()
    DeviceKitHostApiSetup.setUp(
      binaryMessenger: registrar.messenger,
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
    runtimeDriver.launchApp(bundleIdentifier: packageName)
  }

  func dumpUi() throws -> UiSnapshot {
    try runtimeDriver.dumpUi()
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

private final class MacOSAccessibilityDriver {
  private var generation: Int64 = 0
  private var application: AXUIElement?
  private var runningApplication: NSRunningApplication?
  private var elementsByNodeId: [String: AXUIElement] = [:]

  func initialize(config: DriverConfig) {
    generation = 0
    application = nil
    runningApplication = nil
    elementsByNodeId.removeAll()
    if config.enableLogs {
      NSLog("DeviceKit macOS AX session started: %@", config.sessionId)
    }
  }

  func dispose() {
    application = nil
    runningApplication = nil
    elementsByNodeId.removeAll()
  }

  func getDeviceInfo() -> DeviceInfo {
    let version = ProcessInfo.processInfo.operatingSystemVersion
    let osVersion = "\(version.majorVersion).\(version.minorVersion).\(version.patchVersion)"
    return DeviceInfo(
      platform: "macOS",
      osVersion: osVersion,
      model: nil,
      deviceName: Host.current().localizedName,
      physicalDevice: true
    )
  }

  func openAccessibilitySettings() -> ActionResult {
    guard let url = URL(
      string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
    ) else {
      return failure("Unable to create the macOS Accessibility settings URL.")
    }
    NSWorkspace.shared.open(url)
    return success(changed: false)
  }

  func launchApp(bundleIdentifier: String) -> ActionResult {
    let running = NSRunningApplication.runningApplications(
      withBundleIdentifier: bundleIdentifier
    ).first
    let launched = running ?? launch(bundleIdentifier: bundleIdentifier)
    guard let app = launched else {
      return failure("Unable to launch macOS application \(bundleIdentifier).")
    }
    runningApplication = app
    application = AXUIElementCreateApplication(app.processIdentifier)
    app.activate(options: [.activateAllWindows, .activateIgnoringOtherApps])
    return success(changed: false)
  }

  func dumpUi() throws -> UiSnapshot {
    guard AXIsProcessTrusted() else {
      throw accessibilityError(
        "macOS Accessibility permission is required. Enable the host app "
          + "in System Settings > Privacy & Security > Accessibility."
      )
    }

    activateTargetApplication()
    let app = currentApplication()
    generation += 1
    elementsByNodeId.removeAll()
    var nodes: [UiNode] = []
    let (windowsError, rawWindows) = copyAttribute(app, kAXWindowsAttribute)
    let windows = rawWindows.map(axElements(from:)) ?? []
    if windows.isEmpty {
      var processIdentifier: pid_t = 0
      AXUIElementGetPid(app, &processIdentifier)
      throw accessibilityError(
        "No accessible macOS windows found for process \(processIdentifier) "
          + "(AX error \(windowsError.rawValue))."
      )
    }
    for window in windows {
      _ = append(window, parentNodeId: nil, to: &nodes)
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
    guard let element = elementsByNodeId[nodeId] else {
      return failure("The UI node is no longer available.")
    }

    switch action {
    case .press:
      return perform(element, action: kAXPressAction)
    case .focus:
      return setAttribute(element, name: kAXFocusedAttribute, value: true)
    case .setValue:
      guard let value else { return failure("A value is required for setValue.") }
      return setAttribute(element, name: kAXValueAttribute, value: value)
    case .scrollForward:
      return perform(element, action: kAXIncrementAction)
    case .scrollBackward:
      return perform(element, action: kAXDecrementAction)
    }
  }

  func tap(x: Double, y: Double) -> ActionResult {
    activateTargetApplication()
    var element: AXUIElement?
    let error = AXUIElementCopyElementAtPosition(
      AXUIElementCreateSystemWide(),
      Float(x),
      Float(y),
      &element
    )
    guard error == .success, let element else {
      return failure("No accessible macOS element exists at (\(x), \(y)).")
    }
    return perform(element, action: kAXPressAction)
  }

  func swipe(
    fromX: Double,
    fromY: Double,
    toX: Double,
    toY: Double,
    durationMs: Int64
  ) -> ActionResult {
    _ = (fromX, fromY, toX, toY, durationMs)
    return failure("Coordinate swipes are not supported by the macOS AX bridge yet.")
  }

  func typeText(text: String) -> ActionResult {
    activateTargetApplication()
    let app = currentApplication()
    guard let focused = axElementAttribute(app, kAXFocusedUIElementAttribute) else {
      return failure("No focused macOS accessibility element is available.")
    }
    return setAttribute(focused, name: kAXValueAttribute, value: text)
  }

  func pressBack() -> ActionResult {
    activateTargetApplication()
    let app = currentApplication()
    guard let focused = axElementAttribute(app, kAXFocusedUIElementAttribute) else {
      return failure("No focused macOS accessibility element is available.")
    }
    return perform(focused, action: kAXCancelAction)
  }

  func pressHome() -> ActionResult {
    failure("macOS does not expose a semantic Home action through AX.")
  }

  func screenshot() -> FlutterStandardTypedData {
    guard let image = CGDisplayCreateImage(CGMainDisplayID()) else {
      return FlutterStandardTypedData(bytes: Data())
    }
    let bitmap = NSBitmapImageRep(cgImage: image)
    let data = bitmap.representation(using: .png, properties: [:]) ?? Data()
    return FlutterStandardTypedData(bytes: data)
  }

  func requestScreenCapture() -> ActionResult {
    guard AXIsProcessTrusted() else {
      return failure(
        "macOS Accessibility permission is required before controlling other apps."
      )
    }
    return success(changed: false)
  }

  func getClipboard() -> String? {
    NSPasteboard.general.string(forType: .string)
  }

  func setClipboard(text: String) -> ActionResult {
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(text, forType: .string)
    return success(changed: false)
  }

  private func currentApplication() -> AXUIElement {
    if let application {
      return application
    }
    if let frontmost = NSWorkspace.shared.frontmostApplication {
      let element = AXUIElementCreateApplication(frontmost.processIdentifier)
      application = element
      return element
    }
    return AXUIElementCreateSystemWide()
  }

  private func activateTargetApplication() {
    runningApplication?.activate(options: [.activateAllWindows, .activateIgnoringOtherApps])
  }

  private func launch(bundleIdentifier: String) -> NSRunningApplication? {
    guard NSWorkspace.shared.launchApplication(
      withBundleIdentifier: bundleIdentifier,
      options: [],
      additionalEventParamDescriptor: nil,
      launchIdentifier: nil
    ) else {
      return nil
    }
    return NSRunningApplication.runningApplications(
      withBundleIdentifier: bundleIdentifier
    ).first
  }

  private func append(
    _ element: AXUIElement,
    parentNodeId: String?,
    to nodes: inout [UiNode]
  ) -> String {
    let nodeId = "macos-\(generation)-\(nodes.count)"
    elementsByNodeId[nodeId] = element

    let childElements = children(of: element, attribute: kAXChildrenAttribute)
    let role = stringAttribute(element, kAXRoleAttribute) ?? "AXUnknown"
    let bounds = bounds(of: element)
    let nodeIndex = nodes.count
    nodes.append(
      UiNode(
        nodeId: nodeId,
        parentNodeId: parentNodeId,
        childNodeIds: [],
        automationId: stringAttribute(element, kAXIdentifierAttribute),
        text: stringAttribute(element, kAXTitleAttribute),
        label: stringAttribute(element, kAXDescriptionAttribute),
        value: stringValue(attribute(element, kAXValueAttribute)),
        role: normalizedRole(role),
        bounds: RectData(
          x: bounds.origin.x,
          y: bounds.origin.y,
          width: bounds.size.width,
          height: bounds.size.height
        ),
        enabled: boolAttribute(element, kAXEnabledAttribute) ?? true,
        clickable: isClickable(role, element: element),
        editable: isEditable(role),
        focused: boolAttribute(element, kAXFocusedAttribute) ?? false,
        selected: boolAttribute(element, kAXSelectedAttribute) ?? false,
        checked: boolAttribute(element, kAXValueAttribute) ?? false,
        scrollable: role == "AXScrollArea" || role == "AXScrollBar"
      )
    )

    for child in childElements {
      let childNodeId = append(child, parentNodeId: nodeId, to: &nodes)
      nodes[nodeIndex].childNodeIds.append(childNodeId)
    }

    return nodeId
  }

  private func children(of element: AXUIElement, attribute attributeName: String) -> [AXUIElement] {
    guard let value = self.attribute(element, attributeName) else {
      return []
    }
    return axElements(from: value)
  }

  private func axElements(from value: CFTypeRef) -> [AXUIElement] {
    let anyValue: Any = value
    if let elements = anyValue as? [AXUIElement] {
      return elements
    }
    if let values = anyValue as? [Any] {
      return values.compactMap { value in
        guard let object = value as AnyObject? else { return nil }
        return object as! AXUIElement
      }
    }
    guard CFGetTypeID(value) == CFArrayGetTypeID() else { return [] }
    let array = unsafeBitCast(value, to: CFArray.self)
    let count = CFArrayGetCount(array)
    return (0..<count).compactMap { index in
      guard let rawValue = CFArrayGetValueAtIndex(array, index) else {
        return nil
      }
      return unsafeBitCast(rawValue, to: AXUIElement.self)
    }
  }

  private func axElementAttribute(_ element: AXUIElement, _ name: String) -> AXUIElement? {
    guard let value = attribute(element, name) else { return nil }
    return value as! AXUIElement
  }

  private func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    let (_, value) = copyAttribute(element, name)
    return value
  }

  private func copyAttribute(
    _ element: AXUIElement,
    _ name: String
  ) -> (AXError, CFTypeRef?) {
    var value: CFTypeRef?
    let error = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    guard error == .success else { return (error, nil) }
    return (error, value)
  }

  private func stringAttribute(_ element: AXUIElement, _ name: String) -> String? {
    attribute(element, name) as? String
  }

  private func stringValue(_ value: Any?) -> String? {
    guard let value else { return nil }
    if let string = value as? String { return string }
    if let number = value as? NSNumber { return number.stringValue }
    return String(describing: value)
  }

  private func boolAttribute(_ element: AXUIElement, _ name: String) -> Bool? {
    if let value = attribute(element, name) as? Bool { return value }
    return (attribute(element, name) as? NSNumber)?.boolValue
  }

  private func bounds(of element: AXUIElement) -> CGRect {
    var position = CGPoint.zero
    var size = CGSize.zero
    if let value = attribute(element, kAXPositionAttribute) {
      let value = value as! AXValue
      AXValueGetValue(value, .cgPoint, &position)
    }
    if let value = attribute(element, kAXSizeAttribute) {
      let value = value as! AXValue
      AXValueGetValue(value, .cgSize, &size)
    }
    return CGRect(origin: position, size: size)
  }

  private func normalizedRole(_ role: String) -> String {
    switch role {
    case "AXButton": return "button"
    case "AXCheckBox": return "checkbox"
    case "AXRadioButton": return "radio"
    case "AXPopUpButton", "AXComboBox": return "combobox"
    case "AXImage": return "image"
    case "AXLink": return "link"
    case "AXList", "AXTable", "AXOutline": return "list"
    case "AXMenu": return "menu"
    case "AXMenuItem": return "menuItem"
    case "AXProgressIndicator": return "progressBar"
    case "AXSlider": return "slider"
    case "AXTextField", "AXTextArea": return "textField"
    case "AXStaticText": return "text"
    case "AXScrollArea", "AXScrollBar": return "scrollView"
    default: return "unknown"
    }
  }

  private func isClickable(_ role: String, element: AXUIElement) -> Bool {
    if ["AXButton", "AXCheckBox", "AXRadioButton", "AXLink", "AXMenuItem"].contains(role) {
      return true
    }
    return (attribute(element, "AXActionNames") as? [String])?.contains(kAXPressAction) ?? false
  }

  private func isEditable(_ role: String) -> Bool {
    role == "AXTextField" || role == "AXTextArea" || role == "AXComboBox"
  }

  private func perform(_ element: AXUIElement, action: String) -> ActionResult {
    let error = AXUIElementPerformAction(element, action as CFString)
    return error == .success
      ? success(changed: true)
      : failure("AX action \(action) failed with error \(error.rawValue).")
  }

  private func setAttribute(_ element: AXUIElement, name: String, value: Any) -> ActionResult {
    let bridgedValue: CFTypeRef
    if let string = value as? String {
      bridgedValue = string as NSString
    } else if let bool = value as? Bool {
      bridgedValue = (bool ? kCFBooleanTrue : kCFBooleanFalse)
    } else {
      return failure("Unsupported AX value type.")
    }
    let error = AXUIElementSetAttributeValue(element, name as CFString, bridgedValue)
    return error == .success
      ? success(changed: true)
      : failure("AX attribute \(name) could not be set (error \(error.rawValue)).")
  }

  private func accessibilityError(_ message: String) -> NSError {
    NSError(domain: "device_kit_lib.macos.accessibility", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
  }

  private func success(changed: Bool) -> ActionResult {
    ActionResult(success: true, message: nil, uiChanged: changed)
  }

  private func failure(_ message: String) -> ActionResult {
    ActionResult(success: false, message: message, uiChanged: false)
  }
}

#endif

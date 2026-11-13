import Foundation
import AppKit
import ApplicationServices

func failure(_ message: String) -> Never {
  FileHandle.standardError.write(Data((message + "\n").utf8))
  exit(1)
}
func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
  var value: CFTypeRef?
  guard AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success else { return nil }
  return value
}
func text(_ element: AXUIElement, _ key: String) -> String {
  attribute(element, key) as? String ?? ""
}
func bounds(_ element: AXUIElement) -> CGRect? {
  guard let p = attribute(element, kAXPositionAttribute), let s = attribute(element, kAXSizeAttribute),
    CFGetTypeID(p) == AXValueGetTypeID(), CFGetTypeID(s) == AXValueGetTypeID() else { return nil }
  var point = CGPoint.zero
  var size = CGSize.zero
  guard AXValueGetValue(p as! AXValue, .cgPoint, &point), AXValueGetValue(s as! AXValue, .cgSize, &size),
    size.width > 0, size.height > 0 else { return nil }
  return CGRect(origin: point, size: size)
}
func matches(_ title: String, _ name: String) -> Bool {
  title == name || title.hasPrefix(name + " – ") || title.hasPrefix(name + " — ") || title.hasPrefix(name + " (")
}
func windowRows() -> [[String: Any]] {
  CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
}
func rectangle(_ row: [String: Any]) -> CGRect? {
  guard let data = row[kCGWindowBounds as String] as? [String: Double] else { return nil }
  return CGRect(dictionaryRepresentation: data as CFDictionary)
}
if CommandLine.arguments.count > 1 && CommandLine.arguments[1] == "--check" {
  guard CommandLine.arguments.count == 7, let pid = Int32(CommandLine.arguments[2]),
    let id = UInt32(CommandLine.arguments[3]), let width = Double(CommandLine.arguments[5]),
    let height = Double(CommandLine.arguments[6]) else { failure("Invalid source binding.") }
  guard let row = windowRows().first(where: { ($0[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == pid && ($0[kCGWindowNumber as String] as? NSNumber)?.uint32Value == id }),
    matches(row[kCGWindowName as String] as? String ?? "", CommandLine.arguments[4]),
    let frame = rectangle(row), abs(frame.width - width) < 1, abs(frame.height - height) < 1 else {
    failure("The simulator host changed device, closed, or resized. Reopen Live.")
  }
  exit(0)
}
guard CommandLine.arguments.count >= 4, let width = Double(CommandLine.arguments[2]),
  let height = Double(CommandLine.arguments[3]), width > 0, height > 0 else {
  failure("Expected simulator name and logical screen dimensions.")
}
guard AXIsProcessTrusted() else { failure("Accessibility permission is required to identify the simulator screen.") }
let name = CommandLine.arguments[1]
let applications = NSWorkspace.shared.runningApplications.filter {
  ["com.apple.iphonesimulator", "com.apple.dt.DeviceHub", "com.apple.dt.Devices"].contains($0.bundleIdentifier ?? "")
}
let cgWindows = windowRows()
var windows: [(Int32, UInt32, AXUIElement, CGRect)] = []
for application in applications {
  let app = AXUIElementCreateApplication(application.processIdentifier)
  AXUIElementSetMessagingTimeout(app, 0.3)
  for window in attribute(app, kAXWindowsAttribute) as? [AXUIElement] ?? [] {
    guard matches(text(window, kAXTitleAttribute), name), let frame = bounds(window) else { continue }
    let candidates = cgWindows.filter { row in
      guard (row[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == application.processIdentifier,
        let cgFrame = rectangle(row) else { return false }
      return abs(cgFrame.minX-frame.minX)<1 && abs(cgFrame.minY-frame.minY)<1 && abs(cgFrame.width-frame.width)<1 && abs(cgFrame.height-frame.height)<1
    }
    guard candidates.count == 1, let id = (candidates[0][kCGWindowNumber as String] as? NSNumber)?.uint32Value else { continue }
    windows.append((application.processIdentifier,id,window,frame))
  }
}
guard windows.count == 1 else { failure("Open exactly one Simulator or Device Hub window for the selected simulator. Its name must be unique.") }
let (pid, windowID, window, frame) = windows[0]
var screens: [CGRect] = []
func collect(_ node: AXUIElement, _ depth: Int, _ count: inout Int) {
  guard depth < 14, count < 1500 else { return }
  count += 1
  let role = text(node, kAXRoleAttribute)
  let semantic = [text(node,kAXIdentifierAttribute),text(node,kAXDescriptionAttribute)].joined(separator:" ").lowercased()
  if let rect = bounds(node), rect.width >= 80, rect.height >= 80, frame.contains(rect),
    (role == kAXImageRole || semantic.contains("screen") || semantic.contains("display")),
    abs(rect.width/rect.height - width/height) < 0.015,
    !screens.contains(where: { $0.equalTo(rect) }) { screens.append(rect) }
  for child in attribute(node,kAXChildrenAttribute) as? [AXUIElement] ?? [] { collect(child,depth+1,&count) }
}
var crop: CGRect
if CommandLine.arguments.count == 5 {
  guard let data = CommandLine.arguments[4].data(using:.utf8),
    let config = try? JSONSerialization.jsonObject(with:data) as? [String:Any],
    let configuredPID = config["pid"] as? Int32, let configuredWindow = config["windowId"] as? UInt32,
    configuredPID == pid, configuredWindow == windowID,
    let values = config["crop"] as? [Double], values.count == 4, values.allSatisfy({ $0.isFinite }) else {
    failure("Explicit simulator Live target must match this simulator's current PID/window and contain crop [x,y,width,height].")
  }
  crop = CGRect(x:values[0],y:values[1],width:values[2],height:values[3])
  guard CGRect(origin:.zero,size:frame.size).contains(crop),crop.width>=80,crop.height>=80,
    abs(crop.width/crop.height-width/height)<0.015 else { failure("Configured simulator screen crop is outside the selected window or has a different screen aspect ratio.") }
} else {
  var count = 0
  collect(window,0,&count)
  guard screens.count == 1 else { failure("The host does not expose one unambiguous screen accessibility frame. Wait for the simulator display, or configure DEVICE_BOT_SIMULATOR_LIVE_TARGETS with an explicit verified PID/window/crop for this UDID.") }
  let screen=screens[0]
  crop=CGRect(x:screen.minX-frame.minX,y:screen.minY-frame.minY,width:screen.width,height:screen.height)
}
let result: [String:Any] = ["pid":pid,"windowId":windowID,
  "crop":[crop.minX,crop.minY,crop.width,crop.height], "hostWidth":frame.width,"hostHeight":frame.height,
  "width":0,"height":0,"appId":""]
do { FileHandle.standardOutput.write(try JSONSerialization.data(withJSONObject:result)) }
catch { failure("Could not serialize simulator source geometry: \(error)") }

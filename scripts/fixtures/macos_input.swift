import Cocoa

let stateURL = URL(fileURLWithPath: CommandLine.arguments[1])
var state: [String: Any] = ["text": "", "committed": false, "dragMoves": 0]
func save() {
  if let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) {
    try? data.write(to: stateURL, options: .atomic)
  }
}
class DragArea: NSView {
  override var isFlipped: Bool { true }
  override func draw(_ dirtyRect: NSRect) {
    NSColor.systemBlue.setFill()
    bounds.fill()
    ("Drag here" as NSString).draw(
      at: NSPoint(x: 20, y: 20), withAttributes: [.foregroundColor: NSColor.white])
  }
  override func mouseDown(with event: NSEvent) {
    let p = convert(event.locationInWindow, from: nil)
    state["dragStart"] = [p.x, p.y]
    save()
  }
  override func mouseDragged(with event: NSEvent) {
    state["dragMoves"] = (state["dragMoves"] as? Int ?? 0) + 1
    save()
  }
  override func mouseUp(with event: NSEvent) {
    let p = convert(event.locationInWindow, from: nil)
    state["dragEnd"] = [p.x, p.y]
    save()
  }
}
class Delegate: NSObject, NSApplicationDelegate, NSTextFieldDelegate {
  var window: NSWindow!
  func applicationDidFinishLaunching(_ notification: Notification) {
    window = NSWindow(
      contentRect: NSRect(x: 600, y: 350, width: 640, height: 400),
      styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
    window.title = "Device Bot Input Fixture"
    let input = NSTextField(frame: NSRect(x: 20, y: 320, width: 400, height: 30))
    input.placeholderString = "Notes input"
    input.setAccessibilityLabel("Notes input")
    input.delegate = self
    window.contentView!.addSubview(input)
    let button = NSButton(title: "Commit fixture", target: self, action: #selector(commit))
    button.frame = NSRect(x: 440, y: 315, width: 160, height: 40)
    window.contentView!.addSubview(button)
    let disabled = NSTextField(frame: NSRect(x: 20, y: 280, width: 400, height: 30))
    disabled.isEnabled = false
    disabled.setAccessibilityLabel("Disabled input")
    window.contentView!.addSubview(disabled)
    let drag = DragArea(frame: NSRect(x: 20, y: 20, width: 600, height: 240))
    drag.setAccessibilityElement(true)
    drag.setAccessibilityRole(.group)
    drag.setAccessibilityLabel("Drag test area")
    window.contentView!.addSubview(drag)
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    save()
  }
  func controlTextDidChange(_ obj: Notification) {
    state["text"] = (obj.object as? NSTextField)?.stringValue ?? ""
    save()
  }
  @objc func commit() {
    state["committed"] = true
    save()
  }
}
let app = NSApplication.shared
let delegate = Delegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()

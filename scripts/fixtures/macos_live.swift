import Cocoa

// Disposable, animated target for native Live capture and input tests.
let stateURL = URL(fileURLWithPath: CommandLine.arguments[1])
final class LiveDelegate: NSObject, NSApplicationDelegate, NSTextFieldDelegate {
    var window: NSWindow!
    var field: NSTextField!
    var pulse: NSView!
    var timer: Timer?
    var ticks = 0
    var commits = 0

    func save() {
        let state: [String: Any] = [
            "pid": ProcessInfo.processInfo.processIdentifier,
            "window": window.windowNumber,
            "text": field.stringValue,
            "ticks": ticks,
            "commits": commits,
        ]
        if let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) {
            try? data.write(to: stateURL, options: .atomic)
        }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(
            contentRect: NSRect(x: 200, y: 250, width: 640, height: 400),
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "Device Bot Live Fixture"
        field = NSTextField(frame: NSRect(x: 20, y: 320, width: 400, height: 32))
        field.placeholderString = "Live input"
        field.setAccessibilityLabel("Live input")
        field.delegate = self
        window.contentView!.addSubview(field)
        let button = NSButton(title: "Commit fixture", target: self, action: #selector(commit))
        button.frame = NSRect(x: 440, y: 315, width: 170, height: 40)
        window.contentView!.addSubview(button)
        pulse = NSView(frame: NSRect(x: 20, y: 20, width: 600, height: 260))
        pulse.wantsLayer = true
        pulse.layer!.backgroundColor = NSColor.systemBlue.cgColor
        window.contentView!.addSubview(pulse)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        timer = Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] _ in
            guard let self = self else { return }
            self.ticks += 1
            self.pulse.layer!.backgroundColor = (self.ticks % 2 == 0
                ? NSColor.systemBlue : NSColor.systemOrange).cgColor
            self.save()
        }
        save()
    }

    func controlTextDidChange(_ notification: Notification) { save() }
    @objc func commit() { commits += 1; save() }
}

let app = NSApplication.shared
let delegate = LiveDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()

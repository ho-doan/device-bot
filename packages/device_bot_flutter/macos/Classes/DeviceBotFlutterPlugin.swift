import Cocoa
import FlutterMacOS

public class DeviceBotFlutterPlugin: NSObject, FlutterPlugin {
  private weak var view: NSView?
  init(view: NSView?) { self.view = view }

  public static func register(with registrar: FlutterPluginRegistrar) {
    let channel = FlutterMethodChannel(name: "device_bot_flutter/geometry", binaryMessenger: registrar.messenger)
    registrar.addMethodCallDelegate(DeviceBotFlutterPlugin(view: registrar.view), channel: channel)
  }

  public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    guard call.method == "screenOrigin" else { result(FlutterMethodNotImplemented); return }
    guard let view = view, let window = view.window, let screen = NSScreen.screens.first else {
      result(FlutterError(code: "no_view", message: "Flutter view is not attached to a window", details: nil)); return
    }
    let rect = window.convertToScreen(view.convert(view.bounds, to: nil))
    // AX and native input use desktop points, with y measured from the top.
    result([rect.minX, screen.frame.maxY - rect.maxY])
  }
}

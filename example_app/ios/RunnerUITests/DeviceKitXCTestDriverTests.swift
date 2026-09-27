import device_kit_lib_xctest
import XCTest

final class DeviceKitXCTestDriverTests: XCTestCase {
  private let bundleIdentifier = "com.hodoan.devicekit"
  private var driver: DeviceKitXCTestDriver!

  override func setUp() {
    super.setUp()
    continueAfterFailure = false
    driver = DeviceKitXCTestDriver()
    driver.initialize(bundleIdentifier: bundleIdentifier)
  }

  override func tearDown() {
    driver.terminate()
    driver = nil
    super.tearDown()
  }

  func testCounterSemanticActionAndScreenshot() {
    driver.launch()

    let counter = waitForNode { snapshot in
      snapshot.elements.first { $0.automationId == "counter_value" }
    }
    print(
      "DeviceKit counter before: "
        + "id=\(counter.node.automationId ?? "") "
        + "label=\(counter.node.label ?? "") "
        + "text=\(counter.node.text ?? "") "
        + "value=\(counter.node.value ?? "") "
        + "bounds=\(counter.node.bounds)"
    )
    XCTAssertTrue(
      [counter.node.label, counter.node.text, counter.node.value]
        .compactMap { $0 }
        .contains { $0.contains("0") }
    )
    XCTAssertNotNil(counter.node.parentNodeId)
    if let parentNodeId = counter.node.parentNodeId {
      let parent = counter.snapshot.elements.first { $0.nodeId == parentNodeId }
      XCTAssertTrue(parent?.childNodeIds.contains(counter.node.nodeId) == true)
    }
    let button = waitForNode { snapshot in
      snapshot.elements.first { $0.automationId == "increment_button" }
    }
    print(
      "DeviceKit button: "
        + "id=\(button.node.automationId ?? "") "
        + "label=\(button.node.label ?? "") "
        + "bounds=\(button.node.bounds)"
    )

    XCTAssertTrue(
      driver.performElementAction(
        nodeId: button.node.nodeId,
        snapshotGeneration: button.snapshot.generation,
        action: .press
      )
    )

    let updatedCounter = waitForNode { snapshot in
      snapshot.elements.first {
        $0.automationId == "counter_value"
          && [$0.label, $0.text, $0.value]
            .compactMap { $0 }
            .contains { $0.contains("1") }
      }
    }
    print(
      "DeviceKit counter after: "
        + "id=\(updatedCounter.node.automationId ?? "") "
        + "label=\(updatedCounter.node.label ?? "") "
        + "text=\(updatedCounter.node.text ?? "") "
        + "value=\(updatedCounter.node.value ?? "") "
        + "bounds=\(updatedCounter.node.bounds)"
    )

    guard let screenshot = driver.screenshot() else {
      XCTFail("XCUITest screenshot was empty.")
      return
    }
    let attachment = XCTAttachment(
      data: screenshot,
      uniformTypeIdentifier: "public.png"
    )
    attachment.name = "ios-counter-after-increment"
    attachment.lifetime = .keepAlways
    add(attachment)
  }

  private func waitForNode(
    _ predicate: @escaping (DeviceKitXCTestDriver.UiSnapshot) -> DeviceKitXCTestDriver.UiElement?
  ) -> (snapshot: DeviceKitXCTestDriver.UiSnapshot, node: DeviceKitXCTestDriver.UiElement) {
    var match: DeviceKitXCTestDriver.UiElement?
    var matchedSnapshot: DeviceKitXCTestDriver.UiSnapshot?

    let deadline = Date().addingTimeInterval(15)
    while Date() < deadline, match == nil {
      let snapshot = driver.dumpUi()
      match = predicate(snapshot)
      if match != nil {
        matchedSnapshot = snapshot
      }
      if match == nil {
        RunLoop.current.run(until: Date().addingTimeInterval(0.15))
      }
    }
    guard let matchedSnapshot, let match else {
      XCTFail("UI node did not appear before the 15-second timeout.")
      fatalError("UI node did not appear.")
    }
    return (matchedSnapshot, match)
  }
}

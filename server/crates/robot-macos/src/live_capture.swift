import Foundation
import AppKit
import ScreenCaptureKit
import VideoToolbox
import CoreMedia

func fail(_ text: String) -> Never {
    FileHandle.standardError.write(Data((text + "\n").utf8)); exit(1)
}

@available(macOS 12.3, *)
final class Capture: NSObject, SCStreamOutput, SCStreamDelegate {
    var stream: SCStream?
    var encoder: VTCompressionSession?
    var sequence: Int64 = 0
    let queue = DispatchQueue(label: "device-bot.macos.live")

    func start(pid: Int32, windowID: UInt32, width: Int, height: Int, crop: CGRect?) async throws {
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
        guard let window = content.windows.first(where: { $0.windowID == windowID && $0.owningApplication?.processID == pid }) else {
            fail("Selected window is unavailable or belongs to another app; reopen Live.")
        }
        let configuration = SCStreamConfiguration()
        configuration.width = width; configuration.height = height
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 30)
        configuration.queueDepth = 3
        configuration.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        configuration.showsCursor = true
        if let crop = crop { configuration.sourceRect = crop }
        if #available(macOS 14.0, *) { configuration.ignoreShadowsSingleWindow = true }
        let status = VTCompressionSessionCreate(allocator: nil, width: Int32(width), height: Int32(height), codecType: kCMVideoCodecType_H264, encoderSpecification: nil, imageBufferAttributes: nil, compressedDataAllocator: nil, outputCallback: { _, _, status, _, sample in
            guard status == noErr, let sample = sample else { fail("VideoToolbox encoding failed: \(status)") }
            output(sample)
        }, refcon: nil, compressionSessionOut: &encoder)
        guard status == noErr, let encoder = encoder else { fail("Could not create H.264 encoder: \(status)") }
        VTSessionSetProperty(encoder, key: kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue)
        VTSessionSetProperty(encoder, key: kVTCompressionPropertyKey_AllowFrameReordering, value: kCFBooleanFalse)
        VTSessionSetProperty(encoder, key: kVTCompressionPropertyKey_ProfileLevel, value: kVTProfileLevel_H264_Baseline_AutoLevel)
        VTSessionSetProperty(encoder, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: 30 as CFNumber)
        VTSessionSetProperty(encoder, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: 30 as CFNumber)
        VTCompressionSessionPrepareToEncodeFrames(encoder)
        stream = SCStream(filter: SCContentFilter(desktopIndependentWindow: window), configuration: configuration, delegate: self)
        try stream!.addStreamOutput(self, type: .screen, sampleHandlerQueue: queue)
        try await stream!.startCapture()
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, sampleBuffer.isValid, let image = sampleBuffer.imageBuffer, let encoder = encoder else { return }
        let status = VTCompressionSessionEncodeFrame(encoder, imageBuffer: image, presentationTimeStamp: CMTime(value: sequence, timescale: 30), duration: CMTime(value: 1, timescale: 30), frameProperties: nil, sourceFrameRefcon: nil, infoFlagsOut: nil)
        sequence += 1
        if status != noErr { fail("VideoToolbox rejected capture frame: \(status)") }
    }
    func stream(_ stream: SCStream, didStopWithError error: Error) { fail("ScreenCaptureKit stopped: \(error)") }
}

let outputLock = NSLock()
var outputStarted = false

func output(_ sample: CMSampleBuffer) {
    guard let format = sample.formatDescription, let block = sample.dataBuffer else { return }
    outputLock.lock()
    defer { outputLock.unlock() }
    // Finish every AU with the NEXT delimiter so even a static window's first
    // frame is available immediately. That delimiter also starts the next AU.
    var bytes = outputStarted ? Data() : Data([0, 0, 0, 1, 9, 0xf0])
    let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: false) as? [[CFString: Any]]
    let keyframe = attachments?.first?[kCMSampleAttachmentKey_NotSync] as? Bool != true
    if keyframe {
        for index in 0..<2 {
            var pointer: UnsafePointer<UInt8>?
            var size = 0; var count = 0; var header: Int32 = 0
            let status = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, parameterSetIndex: index, parameterSetPointerOut: &pointer, parameterSetSizeOut: &size, parameterSetCountOut: &count, nalUnitHeaderLengthOut: &header)
            if status == noErr, let pointer = pointer { bytes.append(contentsOf: [0, 0, 0, 1]); bytes.append(pointer, count: size) }
        }
    }
    var length = 0; var total = 0; var pointer: UnsafeMutablePointer<Int8>?
    guard CMBlockBufferGetDataPointer(block, atOffset: 0, lengthAtOffsetOut: &length, totalLengthOut: &total, dataPointerOut: &pointer) == noErr, let pointer = pointer else { return }
    let data = UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self)
    var offset = 0
    while offset + 4 <= total {
        let size = Int(data[offset]) << 24 | Int(data[offset+1]) << 16 | Int(data[offset+2]) << 8 | Int(data[offset+3])
        offset += 4
        guard size > 0, offset + size <= total else { fail("Invalid H.264 NAL length") }
        bytes.append(contentsOf: [0, 0, 0, 1]); bytes.append(data + offset, count: size); offset += size
    }
    bytes.append(contentsOf: [0, 0, 0, 1, 9, 0xf0])
    outputStarted = true
    do { try FileHandle.standardOutput.write(contentsOf: bytes) } catch { exit(0) }
}

if #available(macOS 12.3, *) {
    // ScreenCaptureKit needs a WindowServer/AppKit connection even for a CLI.
    NSApplication.shared.setActivationPolicy(.accessory)
    let arguments = CommandLine.arguments
    guard (arguments.count == 5 || arguments.count == 9), let pid = Int32(arguments[1]), let id = UInt32(arguments[2]), let width = Int(arguments[3]), let height = Int(arguments[4]), width >= 2, height >= 2 else { fail("Usage: capture PID WINDOW_ID WIDTH HEIGHT") }
    var crop: CGRect? = nil
    if arguments.count == 9 {
        let values = arguments[5...8].compactMap(Double.init)
        guard values.count == 4, values.allSatisfy({ $0.isFinite }), values[0] >= 0, values[1] >= 0, values[2] >= 2, values[3] >= 2 else { fail("Invalid source crop") }
        crop = CGRect(x: values[0], y: values[1], width: values[2], height: values[3])
    }
    let capture = Capture()
    Task { do { try await capture.start(pid: pid, windowID: id, width: width, height: height, crop: crop) } catch { fail("ScreenCaptureKit: \(error)") } }
    RunLoop.main.run()
} else { fail("macOS Live requires macOS 12.3 or newer") }

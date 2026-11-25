import AVFoundation
import CoreMediaIO
import VideoToolbox
import Foundation
import Darwin

// USB screen sources are opt-in. No camera/name-based fallback is permitted.
var allow: UInt32 = 1
var property = CMIOObjectPropertyAddress(
    mSelector: CMIOObjectPropertySelector(kCMIOHardwarePropertyAllowScreenCaptureDevices),
    mScope: CMIOObjectPropertyScope(kCMIOObjectPropertyScopeGlobal), mElement: 0)
let enabled = CMIOObjectSetPropertyData(CMIOObjectID(kCMIOObjectSystemObject), &property, 0, nil, 4, &allow)
func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8)); exit(1)
}
func writeJSON(_ object: Any) {
    guard let data = try? JSONSerialization.data(withJSONObject: object) else { fail("JSON encoding failed") }
    FileHandle.standardOutput.write(data); FileHandle.standardOutput.write(Data([10]))
}
if enabled != noErr { fail("Could not enable USB iOS screen capture (CoreMediaIO \(enabled))") }
let arguments = CommandLine.arguments
let isListing = arguments.count == 2 && arguments[1] == "--list"
guard isListing || (arguments.count == 3 && arguments[1] == "--stream") else {
    fail("Usage: ios-live --list | --stream <exact-device-id>")
}
let selected = isListing ? nil : arguments[2]

func discoverScreens(selected: String?) -> [AVCaptureDevice] {
    let type: AVCaptureDevice.DeviceType
    if #available(macOS 14.0, *) { type = .external } else { type = .externalUnknown }
    let discovery = AVCaptureDevice.DiscoverySession(deviceTypes: [type], mediaType: .muxed, position: .unspecified)
    let deadline = ProcessInfo.processInfo.systemUptime + 5
    while true {
        let devices = discovery.devices
        if let selected, devices.contains(where: { $0.uniqueID == selected }) { return devices }
        if ProcessInfo.processInfo.systemUptime >= deadline { return devices }
        // CMIO registers the screen asynchronously after opt-in. Let the main run loop
        // handle that registration, and don't end inventory early on another phone.
        RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
    }
}

let devices = discoverScreens(selected: selected)
if isListing {
    writeJSON(devices.map { ["id": $0.uniqueID, "name": $0.localizedName, "model": $0.modelID] }); exit(0)
}
let matches = devices.filter { $0.uniqueID == selected }
if matches.isEmpty {
    // No source is different from a camera, permission, or encoder failure.
    // The host can retry discovery in a fresh process after the service restarts.
    FileHandle.standardOutput.write(Data(repeating: 0, count: 8))
    exit(0)
}
guard matches.count == 1, let device = matches.first else { fail("The selected USB iOS screen source is ambiguous") }

final class Capture: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    private var dimensions: (width: Int, height: Int)?
    var encoder: VTCompressionSession?
    var frame: Int64 = 0
    var lastTimestamp: Double?
    func captureOutput(_ output: AVCaptureOutput, didOutput sample: CMSampleBuffer, from connection: AVCaptureConnection) {
        guard let image = CMSampleBufferGetImageBuffer(sample) else { return }
        let width = CVPixelBufferGetWidth(image), height = CVPixelBufferGetHeight(image)
        if let dimensions {
            guard width == dimensions.width && height == dimensions.height else {
                fail("The iOS screen orientation or resolution changed; reopen Live before sending input")
            }
        } else {
            guard (2...16384).contains(width), (2...16384).contains(height) else { fail("Invalid native screen dimensions") }
            dimensions = (width, height)
            var header = Data()
            withUnsafeBytes(of: UInt32(width).bigEndian) { header.append(contentsOf: $0) }
            withUnsafeBytes(of: UInt32(height).bigEndian) { header.append(contentsOf: $0) }
            FileHandle.standardOutput.write(header)
        }
        let sampleTime = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample))
        let timestamp = sampleTime.isFinite ? sampleTime : ProcessInfo.processInfo.systemUptime
        if let previous = lastTimestamp, timestamp >= previous, timestamp - previous < 1.0 / 30.0 { return }
        lastTimestamp = timestamp
        if encoder == nil {
            let status = VTCompressionSessionCreate(allocator: kCFAllocatorDefault,
                width: Int32(width), height: Int32(height), codecType: kCMVideoCodecType_H264,
                encoderSpecification: nil, imageBufferAttributes: nil, compressedDataAllocator: nil,
                outputCallback: { _, _, status, _, sample in
                    guard status == noErr, let sample, CMSampleBufferDataIsReady(sample),
                          let description = CMSampleBufferGetFormatDescription(sample),
                          let block = CMSampleBufferGetDataBuffer(sample) else { return }
                    var data = Data([0,0,0,1,9,0xf0]) // Access unit delimiter for the Rust media gateway.
                    let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: false) as? [[CFString: Any]]
                    let keyframe = attachments?.first?[kCMSampleAttachmentKey_NotSync] as? Bool != true
                    if keyframe {
                        for index in 0..<2 {
                            var pointer: UnsafePointer<UInt8>?, size = 0, count = 0, header: Int32 = 0
                            if CMVideoFormatDescriptionGetH264ParameterSetAtIndex(description, parameterSetIndex: index,
                                parameterSetPointerOut: &pointer, parameterSetSizeOut: &size,
                                parameterSetCountOut: &count, nalUnitHeaderLengthOut: &header) == noErr, let pointer {
                                data.append(contentsOf: [0,0,0,1]); data.append(pointer, count: size)
                            }
                        }
                    }
                    let length = CMBlockBufferGetDataLength(block)
                    var bytes = [UInt8](repeating: 0, count: length)
                    guard CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: length, destination: &bytes) == noErr else { return }
                    var offset = 0
                    while offset + 4 <= length {
                        let size = Int(bytes[offset]) << 24 | Int(bytes[offset+1]) << 16 | Int(bytes[offset+2]) << 8 | Int(bytes[offset+3])
                        offset += 4
                        guard size > 0, offset + size <= length else { return }
                        data.append(contentsOf: [0,0,0,1]); data.append(contentsOf: bytes[offset..<(offset+size)]); offset += size
                    }
                    FileHandle.standardOutput.write(data)
                }, refcon: nil, compressionSessionOut: &encoder)
            guard status == noErr, let encoder else { fail("VideoToolbox H264 encoder could not start (\(status))") }
            for (key, value) in [(kVTCompressionPropertyKey_RealTime, kCFBooleanTrue!),
                                 (kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse!)] {
                VTSessionSetProperty(encoder, key: key, value: value)
            }
            VTSessionSetProperty(encoder, key: kVTCompressionPropertyKey_ProfileLevel, value: kVTProfileLevel_H264_Baseline_AutoLevel)
            VTSessionSetProperty(encoder, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: 30 as CFNumber)
            VTSessionSetProperty(encoder, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: 30 as CFNumber)
            VTCompressionSessionPrepareToEncodeFrames(encoder)
        }
        guard let encoder else { return }
        let status = VTCompressionSessionEncodeFrame(encoder, imageBuffer: image,
            presentationTimeStamp: CMTime(value: frame, timescale: 30), duration: CMTime(value: 1, timescale: 30),
            frameProperties: nil, sourceFrameRefcon: nil, infoFlagsOut: nil)
        frame += 1
        if status != noErr { fail("VideoToolbox frame encoding failed (\(status))") }
    }
}
let capture = Capture()
let session = AVCaptureSession()
do {
    let input = try AVCaptureDeviceInput(device: device)
    guard session.canAddInput(input) else { fail("The selected iOS screen source cannot be attached") }
    session.addInput(input)
} catch { fail("Cannot open selected iOS screen source: \(error)") }
let output = AVCaptureVideoDataOutput()
output.alwaysDiscardsLateVideoFrames = true
output.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange]
output.setSampleBufferDelegate(capture, queue: DispatchQueue(label: "device-bot.ios-live"))
guard session.canAddOutput(output) else { fail("Cannot attach the iOS screen video output") }
session.addOutput(output)
NotificationCenter.default.addObserver(forName: .AVCaptureSessionRuntimeError, object: session, queue: nil) { notification in
    fail("iOS screen capture stopped: \(notification.userInfo?[AVCaptureSessionErrorKey] ?? "device disconnected")")
}
session.startRunning()
DispatchQueue.global(qos: .utility).async {
    _ = FileHandle.standardInput.readDataToEndOfFile()
    DispatchQueue.main.async {
        session.stopRunning()
        exit(0)
    }
}
RunLoop.main.run()

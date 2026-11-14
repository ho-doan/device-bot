import CoreImage
import Foundation
import ReplayKit
import VideoToolbox

/// ReplayKit supplies screen frames across app switches. No screenshot timer or
/// microphone/app-audio payload is used. One encoded upload is in flight, so
/// slow Wi-Fi drops input frames before encoding, never H.264 reference frames.
final class SampleHandler: RPBroadcastSampleHandler {
    private let lock = NSLock()
    private var busy = false
    private var ended = false
    private var encoder: VTCompressionSession?
    private var pool: CVPixelBufferPool?
    private var dimensions: (Int, Int)?
    private var firstTime: CMTime?
    private var previousPTS: Int64 = -1
    private var configuration: BroadcastConfiguration?
    private var heartbeat: DispatchSourceTimer?
    private let imageContext = CIContext(options: [.cacheIntermediates: false])
    private lazy var network: URLSession = {
        let settings = URLSessionConfiguration.ephemeral
        settings.httpMaximumConnectionsPerHost = 2
        settings.timeoutIntervalForRequest = 6
        settings.timeoutIntervalForResource = 8
        return URLSession(configuration: settings)
    }()

    override func broadcastStarted(withSetupInfo setupInfo: [String: NSObject]?) {
        do {
            configuration = try BroadcastConfiguration.load()
            _ = network
            let timer = DispatchSource.makeTimerSource(queue: .global(qos: .utility))
            timer.schedule(deadline: .now(), repeating: 2)
            timer.setEventHandler { [weak self] in self?.checkConnection() }
            heartbeat = timer
            timer.resume()
        } catch { fail(error.localizedDescription) }
    }

    override func broadcastPaused() { fail("Screen sharing paused. Reopen Live to start a new broadcast.") }
    override func broadcastResumed() {}

    override func broadcastFinished() {
        lock.lock(); ended = true; lock.unlock()
        heartbeat?.cancel()
        heartbeat = nil
        if let encoder {
            VTCompressionSessionInvalidate(encoder)
        }
        encoder = nil
        pool = nil
        if let configuration {
            network.dataTask(with: configuration.request(method: "DELETE")).resume()
        }
        network.finishTasksAndInvalidate()
    }

    override func processSampleBuffer(_ sampleBuffer: CMSampleBuffer, with sampleBufferType: RPSampleBufferType) {
        guard sampleBufferType == .video else { return }
        lock.lock()
        guard !busy && !ended else { lock.unlock(); return }
        busy = true
        lock.unlock()
        do {
            guard let pixels = CMSampleBufferGetImageBuffer(sampleBuffer) else {
                throw BroadcastError.message("ReplayKit did not provide a screen frame.")
            }
            let orientation = (CMGetAttachment(sampleBuffer, key: RPVideoSampleOrientationKey as CFString, attachmentModeOut: nil) as? NSNumber)?.uint32Value ?? 1
            let image = CIImage(cvPixelBuffer: pixels).oriented(CGImagePropertyOrientation(rawValue: orientation) ?? .up)
            let scale = min(1, 1280 / max(image.extent.width, image.extent.height))
            let width = max(2, Int(image.extent.width * scale) / 2 * 2)
            let height = max(2, Int(image.extent.height * scale) / 2 * 2)
            if encoder == nil { try prepareEncoder(width: width, height: height) }
            guard let dimensions, dimensions.0 == width, dimensions.1 == height, let encoder, let pool else {
                throw BroadcastError.message("Screen orientation changed. Reopen Live after rotating your iPhone.")
            }
            let timestamp = CMSampleBufferGetPresentationTimeStamp(sampleBuffer)
            if firstTime == nil { firstTime = timestamp }
            let elapsed = CMTimeGetSeconds(CMTimeSubtract(timestamp, firstTime!))
            guard elapsed.isFinite && elapsed >= 0 else { releaseFrame(); return }
            let pts = Int64(elapsed * 1_000_000)
            guard pts > previousPTS, previousPTS < 0 || pts - previousPTS >= 33_333 else { releaseFrame(); return }
            previousPTS = pts
            var resized: CVPixelBuffer?
            guard CVPixelBufferPoolCreatePixelBuffer(kCFAllocatorDefault, pool, &resized) == kCVReturnSuccess, let resized else {
                throw BroadcastError.message("Cannot allocate the screen encoder buffer.")
            }
            let translated = image.transformed(by: CGAffineTransform(translationX: -image.extent.minX, y: -image.extent.minY))
            let scaled = translated.transformed(by: CGAffineTransform(scaleX: CGFloat(width) / image.extent.width, y: CGFloat(height) / image.extent.height))
            imageContext.render(scaled, to: resized, bounds: CGRect(x: 0, y: 0, width: width, height: height), colorSpace: CGColorSpaceCreateDeviceRGB())
            let status = VTCompressionSessionEncodeFrame(encoder, imageBuffer: resized,
                presentationTimeStamp: CMTime(value: pts, timescale: 1_000_000),
                duration: .invalid, frameProperties: nil, infoFlagsOut: nil) { [weak self] status, flags, sample in
                    guard let self else { return }
                    guard status == noErr else { self.fail("Screen encoding failed (\(status))."); return }
                    guard !flags.contains(.frameDropped), let sample else { self.releaseFrame(); return }
                    do { try self.upload(sample, width: width, height: height, pts: pts) }
                    catch { self.fail(error.localizedDescription) }
                }
            guard status == noErr else { throw BroadcastError.message("Screen encoding failed (\(status)).") }
        } catch { fail(error.localizedDescription) }
    }

    private func prepareEncoder(width: Int, height: Int) throws {
        let status = VTCompressionSessionCreate(allocator: kCFAllocatorDefault,
            width: Int32(width), height: Int32(height), codecType: kCMVideoCodecType_H264,
            encoderSpecification: nil, imageBufferAttributes: nil, compressedDataAllocator: nil,
            outputCallback: nil, refcon: nil, compressionSessionOut: &encoder)
        guard status == noErr, let encoder else {
            throw BroadcastError.message("VideoToolbox H.264 is unavailable (\(status)).")
        }
        let properties: [(CFString, CFTypeRef)] = [
            (kVTCompressionPropertyKey_RealTime, kCFBooleanTrue!),
            (kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse!),
            (kVTCompressionPropertyKey_ProfileLevel, kVTProfileLevel_H264_Baseline_AutoLevel),
            (kVTCompressionPropertyKey_AverageBitRate, 2_500_000 as CFNumber),
            (kVTCompressionPropertyKey_ExpectedFrameRate, 30 as CFNumber),
            (kVTCompressionPropertyKey_MaxKeyFrameInterval, 30 as CFNumber),
            (kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration, 1 as CFNumber),
        ]
        for (key, value) in properties {
            let result = VTSessionSetProperty(encoder, key: key, value: value)
            guard result == noErr else { throw BroadcastError.message("Cannot configure H.264 \(key) (\(result)).") }
        }
        guard VTCompressionSessionPrepareToEncodeFrames(encoder) == noErr else {
            throw BroadcastError.message("Cannot start H.264 encoding.")
        }
        let attributes: [String: Any] = [
            kCVPixelBufferWidthKey as String: width,
            kCVPixelBufferHeightKey as String: height,
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
            kCVPixelBufferIOSurfacePropertiesKey as String: [:],
        ]
        guard CVPixelBufferPoolCreate(kCFAllocatorDefault, nil, attributes as CFDictionary, &pool) == kCVReturnSuccess else {
            throw BroadcastError.message("Cannot create screen encoder buffers.")
        }
        dimensions = (width, height)
    }

    private func upload(_ sample: CMSampleBuffer, width: Int, height: Int, pts: Int64) throws {
        guard let configuration, let description = CMSampleBufferGetFormatDescription(sample),
              let block = CMSampleBufferGetDataBuffer(sample), CMSampleBufferDataIsReady(sample) else {
            throw BroadcastError.message("The screen encoder returned an incomplete frame.")
        }
        let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: false) as? [[CFString: Any]]
        let keyframe = attachments?.first?[kCMSampleAttachmentKey_NotSync] as? Bool != true
        var data = Data([0, 0, 0, 1, 9, 0xf0])
        var nalHeader: Int32 = 0
        for index in 0..<2 {
            var pointer: UnsafePointer<UInt8>?
            var size = 0, count = 0
            let result = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(description, parameterSetIndex: index,
                parameterSetPointerOut: &pointer, parameterSetSizeOut: &size,
                parameterSetCountOut: &count, nalUnitHeaderLengthOut: &nalHeader)
            guard result == noErr, let pointer, nalHeader == 4 else {
                throw BroadcastError.message("The H.264 encoder did not provide valid parameter sets.")
            }
            if keyframe { data.append(contentsOf: [0, 0, 0, 1]); data.append(pointer, count: size) }
        }
        let length = CMBlockBufferGetDataLength(block)
        var bytes = [UInt8](repeating: 0, count: length)
        guard CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: length, destination: &bytes) == noErr else {
            throw BroadcastError.message("Cannot read the encoded screen frame.")
        }
        var offset = 0
        while offset + 4 <= length {
            let size = Int(bytes[offset]) << 24 | Int(bytes[offset+1]) << 16 | Int(bytes[offset+2]) << 8 | Int(bytes[offset+3])
            offset += 4
            guard size > 0 && offset + size <= length else { throw BroadcastError.message("Malformed H.264 access unit.") }
            data.append(contentsOf: [0, 0, 0, 1])
            data.append(contentsOf: bytes[offset..<(offset+size)])
            offset += size
        }
        guard offset == length && data.count <= 2 * 1024 * 1024 else { throw BroadcastError.message("Screen frame is too large.") }
        var request = configuration.request(method: "POST")
        request.setValue("video/h264", forHTTPHeaderField: "Content-Type")
        request.setValue(String(width), forHTTPHeaderField: "X-Video-Width")
        request.setValue(String(height), forHTTPHeaderField: "X-Video-Height")
        request.setValue(String(pts), forHTTPHeaderField: "X-Video-Pts-Us")
        request.httpBody = data
        network.dataTask(with: request) { [weak self] _, response, error in
            guard let self else { return }
            if error != nil { self.fail("Connection to your Mac was lost. Reopen Live to reconnect."); return }
            let status = (response as? HTTPURLResponse)?.statusCode
            guard status == 204 else {
                self.fail(status == 409 ? "Screen geometry changed. Reopen Live after rotating your iPhone." : "Live ended on your Mac. Screen sharing has stopped.")
                return
            }
            self.releaseFrame()
        }.resume()
    }

    private func checkConnection() {
        guard let configuration else { return }
        network.dataTask(with: configuration.request(method: "GET")) { [weak self] _, response, error in
            if error != nil || (response as? HTTPURLResponse)?.statusCode != 204 {
                self?.fail("Live ended or your Mac disconnected. Screen sharing has stopped.")
            }
        }.resume()
    }

    private func releaseFrame() { lock.lock(); busy = false; lock.unlock() }

    private func fail(_ message: String) {
        lock.lock()
        guard !ended else { lock.unlock(); return }
        ended = true
        lock.unlock()
        heartbeat?.cancel()
        NSLog("Device Bot Broadcast: %@", message)
        if let configuration {
            var request = configuration.request(method: "DELETE")
            request.httpBody = Data(message.utf8)
            network.dataTask(with: request).resume()
        }
        DispatchQueue.main.async { [weak self] in
            self?.finishBroadcastWithError(BroadcastError.message(message))
        }
    }
}

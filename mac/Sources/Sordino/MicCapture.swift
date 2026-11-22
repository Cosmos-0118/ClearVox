import AVFoundation

/// Captures one specific mic through AVCaptureSession (so it does NOT depend on the system default
/// input). macOS applies the user's Mic Mode (Voice Isolation) to it.
final class MicCapture: NSObject, AVCaptureAudioDataOutputSampleBufferDelegate {
    static let sampleRate = 48_000.0
    private let session = AVCaptureSession()
    private let queue = DispatchQueue(label: "sordino.capture", qos: .userInteractive)
    private let ring: RingBuffer
    private let onPeak: (Float) -> Void

    init?(deviceUID: String, ring: RingBuffer, onPeak: @escaping (Float) -> Void) {
        self.ring = ring
        self.onPeak = onPeak
        super.init()
        guard let device = AVCaptureDevice(uniqueID: deviceUID),
              let input = try? AVCaptureDeviceInput(device: device), session.canAddInput(input) else { return nil }
        session.addInput(input)
        let out = AVCaptureAudioDataOutput()
        out.audioSettings = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVSampleRateKey: Self.sampleRate,
            AVNumberOfChannelsKey: 1,
            AVLinearPCMBitDepthKey: 32,
            AVLinearPCMIsFloatKey: true,
            AVLinearPCMIsBigEndianKey: false,
            AVLinearPCMIsNonInterleaved: false,
        ]
        out.setSampleBufferDelegate(self, queue: queue)
        guard session.canAddOutput(out) else { return nil }
        session.addOutput(out)
    }

    func start() { session.startRunning() }
    func stop() { session.stopRunning() }

    func captureOutput(_ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        guard let block = CMSampleBufferGetDataBuffer(sampleBuffer) else { return }
        var length = 0
        var ptr: UnsafeMutablePointer<Int8>?
        guard CMBlockBufferGetDataPointer(block, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length, dataPointerOut: &ptr) == kCMBlockBufferNoErr,
              let ptr else { return }
        let n = length / MemoryLayout<Float>.size
        ptr.withMemoryRebound(to: Float.self, capacity: n) { p in
            ring.write(p, n)
            var peak: Float = 0
            for i in 0..<n { peak = max(peak, abs(p[i])) }
            onPeak(peak)
        }
    }
}

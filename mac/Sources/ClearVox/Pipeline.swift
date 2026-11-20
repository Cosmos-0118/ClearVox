import AVFoundation
import AppKit
import AudioToolbox
import CoreAudio
import ServiceManagement

/// Drives only the level meter, so 20 Hz updates never re-render the rest of the menu.
final class LevelModel: ObservableObject {
    @Published var level: Float = 0
}

/// Mic -> Apple voice processing (VPIO) -> [high-pass, limiter] -> virtual mic (BlackHole).
///
/// Threading: UI state lives on the main thread. All AVAudioEngine / CoreAudio work runs on the
/// serial `q`, so toggling or switching devices never blocks the UI.
enum Mode: String, CaseIterable, Identifiable {
    case voiceIsolation = "Voice Isolation"
    case voiceProcessing = "Voice Processing"
    var id: String { rawValue }
}

final class Pipeline: ObservableObject {
    @Published var isRunning = false
    @Published var status: String?
    @Published var devices: [AudioDevice] = []
    @Published var micMode = "Standard"
    let meter = LevelModel()

    @Published var inputUID: String { didSet { if inputUID != oldValue { save(); restartIfRunning() } } }
    @Published var outputUID: String { didSet { if outputUID != oldValue { save(); restartIfRunning() } } }
    @Published var mode: Mode { didSet { if mode != oldValue { save(); restartIfRunning() } } }
    @Published var highPass: Bool { didSet { save(); let v = highPass; q.async { self.eq?.bypass = !v } } }
    @Published var limiter: Bool { didSet { save(); let v = limiter; q.async { self.limiterUnit?.bypass = !v } } }
    @Published var agc: Bool { didSet { save(); let v = agc; q.async { self.capture?.inputNode.isVoiceProcessingAGCEnabled = v } } }
    @Published var useAsDefault: Bool {
        didSet {
            save()
            guard isRunning, mode == .voiceIsolation else { return }
            let on = useAsDefault
            q.async { on ? self.applyDefaultInput() : self.restoreDefaultInput() }
        }
    }
    @Published var launchAtLogin: Bool {
        didSet {
            do {
                if launchAtLogin { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
            } catch { status = "Launch at login failed: \(error.localizedDescription)" }
        }
    }

    // Owned by `q`.
    private let q = DispatchQueue(label: "clearvox.audio", qos: .userInteractive)
    private var capture: AVAudioEngine?
    private var playback: AVAudioEngine?
    private var eq: AVAudioUnitEQ?
    private var limiterUnit: AVAudioUnitEffect?
    private var observers: [NSObjectProtocol] = []
    private var ignoreConfigUntil = Date.distantPast
    private var micCapture: MicCapture?
    private var ringRef: RingBuffer?
    private var routedOutput: AudioDeviceID?

    private var rawLevel: Float = 0     // written by audio thread, read by UI timer
    private var tick = 0
    private let d = UserDefaults.standard

    var inputs: [AudioDevice] { devices.filter { $0.hasInput && !$0.isVirtualLoopback && !$0.isInternalAggregate } }
    var outputs: [AudioDevice] { devices.filter { $0.hasOutput && !$0.isInternalAggregate } }
    var blackHole: AudioDevice? { devices.first { $0.name.lowercased().contains("blackhole") && $0.hasOutput } }

    init() {
        inputUID = d.string(forKey: "inputUID") ?? ""
        outputUID = d.string(forKey: "outputUID") ?? ""
        mode = Mode(rawValue: d.string(forKey: "mode") ?? "") ?? .voiceIsolation
        highPass = d.object(forKey: "highPass") as? Bool ?? true
        limiter = d.object(forKey: "limiter") as? Bool ?? true
        agc = d.object(forKey: "agc") as? Bool ?? true
        useAsDefault = d.object(forKey: "useAsDefault") as? Bool ?? true
        launchAtLogin = SMAppService.mainApp.status == .enabled
        refreshDevices()
        AudioDevices.observeChanges { [weak self] in self?.refreshDevices() }
        AudioDevices.observeDefaultInput { [weak self] in self?.defaultInputChanged() }

        NotificationCenter.default.addObserver(forName: NSApplication.willTerminateNotification, object: nil, queue: nil) { [weak self] _ in
            guard let self else { return }
            self.q.sync { self.restoreDefaultInput() }
        }
        // .common so the meter keeps updating while the menu is open (menu tracking runs a non-default mode).
        let timer = Timer(timeInterval: 1.0 / 20, repeats: true) { [weak self] _ in self?.uiTick() }
        RunLoop.main.add(timer, forMode: .common)

        if CommandLine.arguments.contains("--selftest") { selfTest() }
        else if d.object(forKey: "enabled") as? Bool ?? true { start() }
    }

    private func uiTick() {
        tick += 1
        let l = isRunning ? rawLevel : 0
        if abs(l - meter.level) > 0.002 || (l == 0 && meter.level != 0) { meter.level = l }
        if tick % 20 == 0 {
            if isRunning { checkRouting() }
            let m = Self.micModeName()
            if m != micMode { micMode = m }
        }
    }

    private func save() {
        d.set(inputUID, forKey: "inputUID"); d.set(outputUID, forKey: "outputUID")
        d.set(highPass, forKey: "highPass"); d.set(limiter, forKey: "limiter"); d.set(agc, forKey: "agc")
        d.set(useAsDefault, forKey: "useAsDefault"); d.set(mode.rawValue, forKey: "mode")
    }

    func refreshDevices() {
        let all = AudioDevices.all()
        if all != devices { devices = all }
        if !inputs.contains(where: { $0.uid == inputUID }) {
            inputUID = inputs.first { $0.name.contains("MacBook") }?.uid ?? inputs.first?.uid ?? ""
        }
        if !outputs.contains(where: { $0.uid == outputUID }) {
            outputUID = blackHole?.uid ?? ""
        }
    }

    // MARK: control (main thread)

    func toggle() { isRunning ? stop() : start() }

    func start(persist: Bool = true) {
        if persist { d.set(true, forKey: "enabled") }
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized: launch()
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .audio) { ok in
                DispatchQueue.main.async { ok ? self.launch() : (self.status = "Microphone access denied") }
            }
        default:
            isRunning = false
            status = "Microphone access denied. Enable ClearVox in System Settings > Privacy & Security > Microphone."
        }
    }

    func stop(persist: Bool = true) {
        if persist { d.set(false, forKey: "enabled") }
        isRunning = false
        status = nil
        q.async { self.teardown(restoreDefault: true) }
    }

    private func restartIfRunning() { if isRunning { launch() } }

    private func launch() {
        isRunning = true            // optimistic: UI responds instantly
        status = nil
        let inID = inputs.first { $0.uid == inputUID }?.id
        let outID = outputs.first { $0.uid == outputUID }?.id
        let cfg = Config(inputID: inID, inputUID: inputs.first { $0.uid == inputUID }?.uid, outputID: outID, mode: mode, agc: agc, highPass: highPass, limiter: limiter, useDefault: useAsDefault)
        q.async {
            let error = self.build(cfg)
            if let error { DispatchQueue.main.async { self.isRunning = false; self.status = error } }
        }
    }

    // MARK: engines (queue `q`)

    private struct Config {
        let inputID: AudioDeviceID?, inputUID: String?, outputID: AudioDeviceID?, mode: Mode
        let agc: Bool, highPass: Bool, limiter: Bool, useDefault: Bool
    }

    private func build(_ cfg: Config) -> String? {
        teardown(restoreDefault: false)
        guard let outID = cfg.outputID else {
            return "No virtual mic found. Install BlackHole: brew install --cask blackhole-2ch"
        }
        do {
            var cap: AVAudioEngine?
            var rate = MicCapture.sampleRate
            var mic: MicCapture?
            var ring: RingBuffer!
            let onPeak: (Float) -> Void = { [weak self] peak in
                guard let self else { return }
                self.rawLevel = max(peak, self.rawLevel * 0.85)
            }
            switch cfg.mode {
            case .voiceIsolation:
                // Explicit mic via AVCaptureSession: independent of the system default input,
                // so the virtual mic can safely be the default. macOS Mic Mode applies.
                ring = RingBuffer(sampleRate: rate)
                guard let uid = cfg.inputUID, let m = MicCapture(deviceUID: uid, ring: ring, onPeak: onPeak) else {
                    throw NSError(domain: "ClearVox", code: 2, userInfo: [NSLocalizedDescriptionKey: "Couldn't open the microphone"])
                }
                mic = m
            case .voiceProcessing:
                // Classic Apple voice processing (echo cancel + AGC). It binds to the system default
                // input, so the virtual mic can't be the default in this mode. Switch the default
                // BEFORE creating the engine: VPIO binds to whatever is default at creation, and
                // binding to BlackHole would loop the virtual mic back into itself.
                ignoreConfigUntil = Date().addingTimeInterval(2)
                useAsSystemInput(cfg.inputID)
                if let cur = AudioDevices.defaultInput(), AudioDevices.all().first(where: { $0.id == cur })?.isVirtualLoopback ?? false {
                    throw NSError(domain: "ClearVox", code: 3, userInfo: [NSLocalizedDescriptionKey:
                        "Voice Processing needs a real mic as the system input. Set Sound › Input to your microphone."])
                }
                let engine = AVAudioEngine()
                let input = engine.inputNode
                try input.setVoiceProcessingEnabled(true)
                input.isVoiceProcessingAGCEnabled = cfg.agc
                if #available(macOS 14.0, *) {
                    input.voiceProcessingOtherAudioDuckingConfiguration =
                        AVAudioVoiceProcessingOtherAudioDuckingConfiguration(enableAdvancedDucking: false, duckingLevel: .min)
                }
                let fmt = input.outputFormat(forBus: 0)
                guard fmt.sampleRate > 0 else { throw NSError(domain: "ClearVox", code: 1, userInfo: [NSLocalizedDescriptionKey: "Input device unavailable"]) }
                rate = fmt.sampleRate
                ring = RingBuffer(sampleRate: rate)
                let r = ring!
                input.installTap(onBus: 0, bufferSize: 512, format: fmt) { buf, _ in
                    guard let ch = buf.floatChannelData else { return }
                    let n = Int(buf.frameLength)
                    r.write(ch[0], n)
                    var peak: Float = 0
                    for i in 0..<n { peak = max(peak, abs(ch[0][i])) }
                    onPeak(peak)
                }
                cap = engine
            }
            let ring0 = ring!
            ringRef = ring0

            let pb = AVAudioEngine()
            let mono = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 1)!
            let src = AVAudioSourceNode(format: mono) { _, _, frames, abl in
                let list = UnsafeMutableAudioBufferListPointer(abl)
                guard let p = list[0].mData?.assumingMemoryBound(to: Float.self) else { return noErr }
                ring0.read(p, Int(frames))
                return noErr
            }
            let eq = AVAudioUnitEQ(numberOfBands: 1)
            let band = eq.bands[0]
            band.filterType = .highPass; band.frequency = 90; band.bandwidth = 0.7; band.bypass = false
            eq.bypass = !cfg.highPass
            let lim = AVAudioUnitEffect(audioComponentDescription: AudioComponentDescription(
                componentType: kAudioUnitType_Effect, componentSubType: kAudioUnitSubType_PeakLimiter,
                componentManufacturer: kAudioUnitManufacturer_Apple, componentFlags: 0, componentFlagsMask: 0))
            lim.bypass = !cfg.limiter
            pb.attach(src); pb.attach(eq); pb.attach(lim)
            pb.connect(src, to: eq, format: mono)
            pb.connect(eq, to: lim, format: mono)
            pb.connect(lim, to: pb.mainMixerNode, format: mono)

            // Change the system default input BEFORE starting playback: a device change after
            // start makes AVAudioEngine re-route its output to the default device (the speakers),
            // which creates a speaker -> mic feedback howl.
            if cfg.useDefault && cfg.mode == .voiceIsolation { applyDefaultInput(); Thread.sleep(forTimeInterval: 0.3) }

            // Bind the output to the virtual mic as the last step before start, then verify.
            pb.prepare()
            guard Self.setDevice(pb.outputNode.audioUnit, outID) == noErr else {
                throw NSError(domain: "ClearVox", code: 4, userInfo: [NSLocalizedDescriptionKey: "Couldn't route audio to the virtual mic"])
            }
            try cap?.start()
            mic?.start()
            try pb.start()
            guard Self.boundDevice(pb.outputNode.audioUnit) == outID else {
                pb.stop()
                throw NSError(domain: "ClearVox", code: 5, userInfo: [NSLocalizedDescriptionKey: "Audio wasn't routed to the virtual mic, so ClearVox stopped to avoid feedback"])
            }
            capture = cap; micCapture = mic; playback = pb; self.eq = eq; limiterUnit = lim; routedOutput = outID

            let onConfigChange: (Notification) -> Void = { [weak self] _ in
                guard let self else { return }
                self.q.async {
                    // Never let the processed voice reach real speakers, even for a moment.
                    if !self.playbackIsSafe() { self.playback?.stop() }
                    guard Date() > self.ignoreConfigUntil || self.playback?.isRunning == false else { return }
                    DispatchQueue.main.async { self.restartIfRunning() }
                }
            }
            observers = [NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: pb, queue: nil, using: onConfigChange)]
            if let cap {
                observers.append(NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: cap, queue: nil, using: onConfigChange))
            }
            return nil
        } catch {
            teardown(restoreDefault: false)
            return "Start failed: \(error.localizedDescription)"
        }
    }

    private func teardown(restoreDefault: Bool) {
        observers.forEach(NotificationCenter.default.removeObserver); observers = []
        capture?.inputNode.removeTap(onBus: 0)
        capture?.stop(); playback?.stop()
        capture = nil; playback = nil; eq = nil; limiterUnit = nil; routedOutput = nil
        rawLevel = 0
        micCapture?.stop(); micCapture = nil
        if restoreDefault { restoreDefaultInput() }
    }

    /// Makes the virtual mic the system default input so apps pick it automatically.
    private func applyDefaultInput() {
        let all = AudioDevices.all()
        guard let bh = all.first(where: { $0.name.lowercased().contains("blackhole") && $0.hasInput }) else { return }
        let current = AudioDevices.defaultInput()
        if current != bh.id, let cur = all.first(where: { $0.id == current }) {
            d.set(cur.uid, forKey: "previousDefaultInputUID")
        }
        ignoreConfigUntil = Date().addingTimeInterval(2)
        AudioDevices.setDefaultInput(bh.id)
    }

    /// Puts the user's real default mic back (only if the virtual mic is still the default).
    private func restoreDefaultInput() {
        let all = AudioDevices.all()
        guard let bh = all.first(where: { $0.name.lowercased().contains("blackhole") && $0.hasInput }),
              AudioDevices.defaultInput() == bh.id,
              let uid = d.string(forKey: "previousDefaultInputUID"),
              let prev = all.first(where: { $0.uid == uid && $0.hasInput }) else { return }
        ignoreConfigUntil = Date().addingTimeInterval(2)
        AudioDevices.setDefaultInput(prev.id)
    }

    // MARK: helpers

    /// True when playback is stopped or still bound to the virtual mic it was started on. (queue `q`)
    private func playbackIsSafe() -> Bool {
        guard let pb = playback, pb.isRunning, let want = routedOutput else { return true }
        return Self.boundDevice(pb.outputNode.audioUnit) == want
    }

    /// Once a second: if playback ever drifted onto another device (e.g. the speakers), stop it and rebuild.
    private func checkRouting() {
        q.async {
            guard !self.playbackIsSafe() else { return }
            self.playback?.stop()
            DispatchQueue.main.async { self.restartIfRunning() }
        }
    }

    /// macOS changes the default input on its own when a headset (AirPods, USB, Bluetooth) connects.
    /// Keep it where ClearVox needs it, or apps silently bypass ClearVox / VPIO cleans the wrong mic. (main thread)
    private func defaultInputChanged() {
        guard isRunning else { return }
        let all = AudioDevices.all()
        let current = AudioDevices.defaultInput()
        switch mode {
        case .voiceIsolation:
            guard useAsDefault else { return }
            let bh = all.first { $0.name.lowercased().contains("blackhole") && $0.hasInput }
            guard let bh, current != bh.id else { return }
            q.async { self.applyDefaultInput() }
        case .voiceProcessing:
            guard let want = inputs.first(where: { $0.uid == inputUID })?.id, current != want else { return }
            // VPIO follows the default input, so put the chosen mic back and rebuild on it.
            q.async {
                AudioDevices.setDefaultInput(want)
                DispatchQueue.main.async { self.restartIfRunning() }
            }
        }
    }

    /// Voice Processing: VPIO always captures the system default input, so make that the mic chosen in the menu.
    private func useAsSystemInput(_ id: AudioDeviceID?) {
        guard let id else { ensureRealDefault(preferred: nil); return }
        if AudioDevices.defaultInput() != id {
            AudioDevices.setDefaultInput(id)
            Thread.sleep(forTimeInterval: 0.2)
        }
    }

    private func ensureRealDefault(preferred: AudioDeviceID?) {
        let all = AudioDevices.all()
        guard let cur = AudioDevices.defaultInput(), all.first(where: { $0.id == cur })?.isVirtualLoopback ?? true else { return }
        let prevUID = d.string(forKey: "previousDefaultInputUID")
        let real = all.first(where: { $0.uid == prevUID && $0.hasInput && !$0.isVirtualLoopback })?.id ?? preferred
        if let real { AudioDevices.setDefaultInput(real); Thread.sleep(forTimeInterval: 0.2) }
    }

    @discardableResult
    private static func setDevice(_ unit: AudioUnit?, _ id: AudioDeviceID) -> OSStatus {
        guard let unit else { return -1 }
        var dev = id
        return AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, &dev, UInt32(MemoryLayout<AudioDeviceID>.size))
    }

    private static func boundDevice(_ unit: AudioUnit?) -> AudioDeviceID? {
        guard let unit else { return nil }
        var dev = AudioDeviceID(0); var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        guard AudioUnitGetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, &dev, &size) == noErr else { return nil }
        return dev
    }

    private static func micModeName() -> String {
        switch AVCaptureDevice.activeMicrophoneMode {
        case .voiceIsolation: return "Voice Isolation"
        case .wideSpectrum: return "Wide Spectrum"
        default: return "Standard"
        }
    }

    /// Opens the macOS Mic Mode picker (Voice Isolation / Wide Spectrum / Standard).
    func showMicModes() { AVCaptureDevice.showSystemUserInterface(.microphoneModes) }

    /// `ClearVox --selftest`: prints what the pipeline is doing for 8 s, then exits.
    private func selfTest() {
        start(persist: false)
        var n = 0
        Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] t in
            guard let self else { return }
            n += 1
            let all = AudioDevices.all()
            let name = { (id: AudioDeviceID?) in all.first { $0.id == id }?.name ?? "?" }
            let bound = self.q.sync { Self.boundDevice(self.capture?.inputNode.audioUnit) }
            print("t=\(n) running=\(self.isRunning) peak=\(String(format: "%.6f", self.rawLevel)) engineInput=\(name(bound)) defaultInput=\(name(AudioDevices.defaultInput())) status=\(self.status ?? "-") under=\(self.ringRef?.underruns ?? -1) over=\(self.ringRef?.overflows ?? -1) wN=\(self.ringRef?.lastWriteN ?? -1) rN=\(self.ringRef?.lastReadN ?? -1)")
            fflush(stdout)
            if n >= 8 { self.stop(persist: false); t.invalidate(); exit(0) }
        }
    }
}

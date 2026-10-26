// Long-lived helper that runs Apple's on-device DictationTranscriber
// (SpeechAnalyzer, macOS 26+) for the Rust app. Protocol: see README.md in
// src-tauri/src/apple_speech/.
@preconcurrency import AVFoundation
import Darwin
import Foundation
import Speech

struct Request: Decodable {
    let id: UInt64
    let op: String
    let locale: String?
    let samples: Int?
    let words: [String]?
    let download: Bool?
}

final class Stdin {
    private var buffer = [UInt8]()
    private var start = 0

    private func fill() -> Bool {
        var chunk = [UInt8](repeating: 0, count: 1 << 16)
        let n = chunk.withUnsafeMutableBytes { read(0, $0.baseAddress, $0.count) }
        if n <= 0 { return false }
        if start > 0 {
            buffer.removeFirst(start)
            start = 0
        }
        buffer.append(contentsOf: chunk[0..<n])
        return true
    }

    func line() -> Data? {
        while true {
            if let nl = buffer[start...].firstIndex(of: 0x0A) {
                let data = Data(buffer[start..<nl])
                start = nl + 1
                return data
            }
            if !fill() { return nil }
        }
    }

    func bytes(_ count: Int) -> [UInt8]? {
        while buffer.count - start < count {
            if !fill() { return nil }
        }
        let out = Array(buffer[start..<start + count])
        start += count
        return out
    }
}

func send(_ object: [String: Any]) {
    guard var data = try? JSONSerialization.data(withJSONObject: object) else { return }
    data.append(0x0A)
    FileHandle.standardOutput.write(data)
}

struct HelperError: Error {
    let code: String
    let message: String
}

@available(macOS 26.0, *)
actor Engine {
    private var formats: [String: AVAudioFormat] = [:]
    private var ready: Set<String> = []

    private func transcriber(_ locale: Locale, long: Bool = false) -> DictationTranscriber {
        DictationTranscriber(locale: locale, preset: long ? .longDictation : .shortDictation)
    }

    func prepare(_ identifier: String, download: Bool) async throws -> Locale {
        guard let locale = await DictationTranscriber.supportedLocale(
            equivalentTo: Locale(identifier: identifier))
        else {
            throw HelperError(code: "unsupported_locale",
                              message: "Apple dictation does not support \(identifier)")
        }
        let key = locale.identifier
        if ready.contains(key) { return locale }
        let module = transcriber(locale)
        if await AssetInventory.status(forModules: [module]) != .installed,
           await DictationTranscriber.installedLocales.contains(locale) {
            try await AssetInventory.reserve(locale: locale)
        }
        if await AssetInventory.status(forModules: [module]) != .installed {
            guard download,
                  let request = try await AssetInventory.assetInstallationRequest(supporting: [module])
            else {
                throw HelperError(code: "assets_missing",
                                  message: "Speech assets for \(key) are not installed")
            }
            try await request.downloadAndInstall()
        }
        if let format = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [module]) {
            formats[key] = format
        }
        ready.insert(key)
        return locale
    }

    func transcribe(_ samples: [Float], identifier: String, words: [String]) async throws -> String {
        let locale = try await prepare(identifier, download: false)
        let module = transcriber(locale, long: samples.count > 16_000 * 60)
        let options = SpeechAnalyzer.Options(priority: .userInitiated, modelRetention: .processLifetime)
        let analyzer = SpeechAnalyzer(modules: [module], options: options)
        if !words.isEmpty {
            let context = AnalysisContext()
            context.contextualStrings[.general] = words
            try await analyzer.setContext(context)
        }
        let buffer = try pcmBuffer(samples, to: formats[locale.identifier])
        try await analyzer.prepareToAnalyze(in: buffer.format)

        let collector = Task {
            var text = ""
            for try await result in module.results {
                text += String(result.text.characters)
            }
            return text
        }
        let (stream, input) = AsyncStream<AnalyzerInput>.makeStream()
        input.yield(AnalyzerInput(buffer: buffer))
        input.finish()
        try await analyzer.start(inputSequence: stream)
        try await analyzer.finalizeAndFinishThroughEndOfInput()
        return try await collector.value.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func pcmBuffer(_ samples: [Float], to target: AVAudioFormat?) throws -> AVAudioPCMBuffer {
        let source = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16_000,
                                   channels: 1, interleaved: false)!
        let frames = AVAudioFrameCount(samples.count)
        guard let input = AVAudioPCMBuffer(pcmFormat: source, frameCapacity: max(frames, 1)) else {
            throw HelperError(code: "failed", message: "Could not allocate an audio buffer")
        }
        input.frameLength = frames
        samples.withUnsafeBufferPointer { src in
            input.floatChannelData![0].update(from: src.baseAddress!, count: samples.count)
        }
        guard let target, target != source else { return input }
        guard let converter = AVAudioConverter(from: source, to: target) else {
            throw HelperError(code: "failed", message: "No converter to \(target)")
        }
        let capacity = AVAudioFrameCount(Double(frames) * target.sampleRate / source.sampleRate) + 1024
        guard let output = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else {
            throw HelperError(code: "failed", message: "Could not allocate an audio buffer")
        }
        var fed = false
        var conversionError: NSError?
        converter.convert(to: output, error: &conversionError) { _, status in
            if fed {
                status.pointee = .endOfStream
                return nil
            }
            fed = true
            status.pointee = .haveData
            return input
        }
        if let conversionError { throw conversionError }
        return output
    }
}

func failure(_ id: UInt64, _ error: Error) -> [String: Any] {
    if let e = error as? HelperError {
        return ["id": id, "ok": false, "code": e.code, "error": e.message]
    }
    return ["id": id, "ok": false, "code": "failed", "error": String(describing: error)]
}

@available(macOS 26.0, *)
func serve(_ reader: Stdin) async {
    let engine = Engine()
    send(["ready": true, "supported": true])
    while let line = reader.line() {
        if line.isEmpty { continue }
        guard let req = try? JSONDecoder().decode(Request.self, from: line) else {
            send(["id": 0, "ok": false, "code": "bad_request", "error": "Unreadable request"])
            continue
        }
        let locale = req.locale ?? "en-IN"
        switch req.op {
        case "prepare":
            do {
                let resolved = try await engine.prepare(locale, download: req.download ?? false)
                send(["id": req.id, "ok": true, "locale": resolved.identifier])
            } catch {
                send(failure(req.id, error))
            }
        case "transcribe":
            let count = req.samples ?? 0
            guard let raw = reader.bytes(count * 4) else { return }
            let samples = raw.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
            let started = Date()
            do {
                let text = try await engine.transcribe(samples, identifier: locale, words: req.words ?? [])
                let ms = Int(Date().timeIntervalSince(started) * 1000)
                send(["id": req.id, "ok": true, "text": text, "ms": ms])
            } catch {
                send(failure(req.id, error))
            }
        default:
            send(["id": req.id, "ok": false, "code": "bad_request", "error": "Unknown op \(req.op)"])
        }
    }
}

let reader = Stdin()
if #available(macOS 26.0, *) {
    await serve(reader)
} else {
    send(["ready": true, "supported": false])
    while let line = reader.line() {
        guard let req = try? JSONDecoder().decode(Request.self, from: line) else { continue }
        if let n = req.samples, reader.bytes(n * 4) == nil { break }
        send(["id": req.id, "ok": false, "code": "unsupported_os",
              "error": "Apple dictation needs macOS 26 or later"])
    }
}

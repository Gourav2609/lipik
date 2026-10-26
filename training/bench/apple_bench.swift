// Usage: swift apple_bench.swift <speech|dictation> <locale> <out.jsonl>
// Transcribes every clip in audio/ with Apple's on-device recognisers (macOS 26).
import AVFoundation
import Foundation
import Speech

let args = CommandLine.arguments
guard args.count == 4 else { print("usage: apple_bench <speech|dictation> <locale> <out.jsonl>"); exit(1) }
let kind = args[1], locale = Locale(identifier: args[2]), outPath = args[3]
let here = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let clips = try! FileManager.default.contentsOfDirectory(at: here.appendingPathComponent("audio"), includingPropertiesForKeys: nil)
    .filter { $0.pathExtension == "wav" }.sorted { $0.lastPathComponent < $1.lastPathComponent }

func transcribe(_ url: URL) async throws -> String {
    let file = try AVAudioFile(forReading: url)
    var text = ""
    if kind == "speech" {
        let t = SpeechTranscriber(locale: locale, preset: .transcription)
        let analyzer = try await SpeechAnalyzer(inputAudioFile: file, modules: [t], finishAfterFile: true)
        _ = analyzer
        for try await r in t.results { text += String(r.text.characters) }
    } else {
        let t = DictationTranscriber(locale: locale, preset: .shortDictation)
        let analyzer = try await SpeechAnalyzer(inputAudioFile: file, modules: [t], finishAfterFile: true)
        _ = analyzer
        for try await r in t.results { text += String(r.text.characters) }
    }
    return text.trimmingCharacters(in: .whitespacesAndNewlines)
}

let sem = DispatchSemaphore(value: 0)
Task {
    do {
        let module: any SpeechModule = kind == "speech"
            ? SpeechTranscriber(locale: locale, preset: .transcription)
            : DictationTranscriber(locale: locale, preset: .shortDictation)
        if let req = try await AssetInventory.assetInstallationRequest(supporting: [module]) {
            print("downloading Apple speech assets for \(locale.identifier)…")
            try await req.downloadAndInstall()
        }
        var lines: [String] = []
        for url in clips {
            let file = try AVAudioFile(forReading: url)
            let secs = Double(file.length) / file.processingFormat.sampleRate
            var best = Double.infinity, text = ""
            for _ in 0..<2 {
                let t0 = Date()
                text = try await transcribe(url)
                best = min(best, Date().timeIntervalSince(t0) * 1000)
            }
            let row: [String: Any] = ["clip": url.deletingPathExtension().lastPathComponent, "text": text,
                                      "audio_secs": secs, "best_ms": Int(best), "load_ms": 0]
            lines.append(String(data: try JSONSerialization.data(withJSONObject: row), encoding: .utf8)!)
        }
        try (lines.joined(separator: "\n") + "\n").write(toFile: outPath, atomically: true, encoding: .utf8)
        print("wrote \(lines.count) clips to \(outPath)")
    } catch {
        print("error: \(error)")
    }
    sem.signal()
}
sem.wait()

// Usage: apple <locale> <list.tsv> <out.jsonl>
// list.tsv has one "id<TAB>/path/to/clip.wav" per line. Transcribes each clip with
// Apple's on-device DictationTranscriber (macOS 26) and appends one JSON line per
// clip to out.jsonl. Ids already in out.jsonl are skipped. Reads files only: no mic.
import AVFoundation
import Foundation
import Speech

let args = CommandLine.arguments
guard args.count == 4 else { print("usage: apple <locale> <list.tsv> <out.jsonl>"); exit(1) }
let locale = Locale(identifier: args[1]), listPath = args[2], outPath = args[3]

let clips: [(String, URL)] = try! String(contentsOfFile: listPath, encoding: .utf8)
    .split(separator: "\n").compactMap { line in
        let parts = line.split(separator: "\t", maxSplits: 1).map(String.init)
        return parts.count == 2 ? (parts[0], URL(fileURLWithPath: parts[1])) : nil
    }
var done = Set<String>()
if let old = try? String(contentsOfFile: outPath, encoding: .utf8) {
    for line in old.split(separator: "\n") {
        if let obj = try? JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any], let id = obj["id"] as? String {
            done.insert(id)
        }
    }
}

func transcribe(_ url: URL) async throws -> String {
    let file = try AVAudioFile(forReading: url)
    let t = DictationTranscriber(locale: locale, preset: .shortDictation)
    let analyzer = try await SpeechAnalyzer(inputAudioFile: file, modules: [t], finishAfterFile: true)
    _ = analyzer
    var text = ""
    for try await r in t.results { text += String(r.text.characters) }
    return text.trimmingCharacters(in: .whitespacesAndNewlines)
}

func append(_ row: [String: Any]) {
    let line = String(data: try! JSONSerialization.data(withJSONObject: row), encoding: .utf8)! + "\n"
    if let h = FileHandle(forWritingAtPath: outPath) {
        h.seekToEndOfFile()
        h.write(line.data(using: .utf8)!)
        h.closeFile()
    } else {
        try! line.write(toFile: outPath, atomically: true, encoding: .utf8)
    }
}

let sem = DispatchSemaphore(value: 0)
Task {
    do {
        let module = DictationTranscriber(locale: locale, preset: .shortDictation)
        if let req = try await AssetInventory.assetInstallationRequest(supporting: [module]) {
            print("downloading Apple speech assets for \(locale.identifier)…")
            try await req.downloadAndInstall()
        }
        var n = 0
        for (id, url) in clips where !done.contains(id) {
            let t0 = Date()
            var row: [String: Any] = ["id": id]
            do {
                row["text"] = try await transcribe(url)
            } catch {
                row["text"] = ""
                row["error"] = "\(error)"
            }
            row["seconds"] = Date().timeIntervalSince(t0)
            append(row)
            n += 1
            print("\(id) \(String(format: "%.2f", row["seconds"] as! Double))s")
        }
        print("wrote \(n) clips to \(outPath)")
    } catch {
        print("error: \(error)")
    }
    sem.signal()
}
sem.wait()
